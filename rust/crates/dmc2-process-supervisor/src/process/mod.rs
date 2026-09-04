use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::raw::{c_int, c_ulong};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::catalog::{self, ProcessRole};
use crate::event::{hex_bytes, Event};

#[derive(Debug, Clone)]
pub struct ProcessIdentity {
    label: String,
    role: Option<ProcessRole>,
    source: &'static str,
}

impl ProcessIdentity {
    pub fn for_role(role: ProcessRole, source: &'static str) -> Self {
        Self {
            label: role.name().to_owned(),
            role: Some(role),
            source,
        }
    }

    pub fn unknown(label: impl Into<String>, source: &'static str) -> Self {
        Self {
            label: label.into(),
            role: None,
            source,
        }
    }

    pub fn role(&self) -> Option<ProcessRole> {
        self.role
    }

    pub fn is_more_specific_than(&self, previous: &Self) -> bool {
        self.role.is_some() && previous.role.is_none()
    }

    pub fn event_fields(&self, event: Event) -> Event {
        let event = event
            .field("role", &self.label)
            .field("identity_source", self.source);
        match self.role {
            Some(role) => event
                .field("catalog_state", "matched")
                .field("catalog_program", role.program())
                .field("catalog_launch_site", role.launch_site())
                .field("catalog_ownership", role.ownership().name())
                .field("catalog_criticality", role.criticality().name())
                .field("catalog_backtrace", role.backtrace().name()),
            None => event.field("catalog_state", "unmatched"),
        }
    }
}

pub fn set_process_owner_identity(role: ProcessRole) -> io::Result<()> {
    const PR_SET_NAME: c_int = 15;
    let name = role.owner_comm().as_bytes();
    let mut buffer = [0_u8; 16];
    buffer[..name.len()].copy_from_slice(name);
    // SAFETY: PR_SET_NAME reads this live NUL-terminated stack buffer and retains no pointer.
    if unsafe { prctl(PR_SET_NAME, buffer.as_ptr() as c_ulong, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let observed = fs::read("/proc/self/comm")?;
    if observed.strip_suffix(b"\n") == Some(name) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "process name mismatch: requested {:?}, observed {:?}",
            role.owner_comm(),
            String::from_utf8_lossy(&observed)
        )))
    }
}

pub fn identify(pid: u32) -> ProcessIdentity {
    let root = PathBuf::from(format!("/proc/{pid}"));
    let executable = fs::read_link(root.join("exe"));
    let cmdline = fs::read(root.join("cmdline")).unwrap_or_default();
    let comm = fs::read(root.join("comm")).ok();
    if let Some(role) = comm
        .as_deref()
        .and_then(|value| catalog::identify_process_owner(value).ok().flatten())
    {
        return ProcessIdentity::for_role(role, "supervisor-process-name");
    }
    if executable.as_ref().ok().and_then(|path| path.file_name())
        == Some(OsStr::new("dmc2-process-supervisor"))
    {
        if let Some(role) = supervisor_role(&cmdline) {
            return ProcessIdentity::for_role(role, "supervisor-command-line-role");
        }
    }
    if let Ok(Some((role, source))) =
        catalog::identify_process(executable.as_deref().ok(), &cmdline, comm.as_deref())
    {
        return ProcessIdentity::for_role(role, source.name());
    }
    if let Some(name) = executable
        .as_ref()
        .ok()
        .and_then(|path| path.file_name())
        .and_then(OsStr::to_str)
    {
        return ProcessIdentity::unknown(format!("uncatalogued:{name}"), "executable-basename");
    }
    if let Some(comm) = comm.as_deref() {
        return ProcessIdentity::unknown(
            format!("unmatched:{}", String::from_utf8_lossy(comm).trim()),
            "process-name-only",
        );
    }
    ProcessIdentity::unknown("unknown", "process-no-longer-readable")
}

pub fn executable_event_fields(event: Event, program: &Path) -> Event {
    let event = match fs::canonicalize(program) {
        Ok(path) => event
            .field("executable_identity_state", "captured")
            .encoded_path_field("executable_canonical_hex", &path),
        Err(error) => event
            .field("executable_identity_state", "canonicalize-failed")
            .field("executable_identity_error", unavailable(&error)),
    };
    match fs::metadata(program) {
        Ok(metadata) => event
            .field("executable_device", metadata.dev())
            .field("executable_inode", metadata.ino())
            .field("executable_size", metadata.size())
            .field("executable_mtime_seconds", metadata.mtime()),
        Err(error) => event.field("executable_metadata_error", unavailable(&error)),
    }
}

pub fn event_fields(event: Event, pid: u32) -> Event {
    let root = PathBuf::from(format!("/proc/{pid}"));
    let event = read_field(
        event.field("child_pid", pid),
        "proc_comm_hex",
        root.join("comm"),
    );
    let event = read_field(event, "proc_cmdline_hex", root.join("cmdline"));
    let event = read_field(event, "proc_stat_hex", root.join("stat"));
    match fs::read_link(root.join("exe")) {
        Ok(path) => event.encoded_path_field("proc_exe_hex", &path),
        Err(error) => event.field("proc_exe_hex", unavailable(&error)),
    }
}

pub fn exists(pid: u32) -> io::Result<bool> {
    match fs::metadata(format!("/proc/{pid}")) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn read_field(event: Event, name: &'static str, path: PathBuf) -> Event {
    match fs::read(path) {
        Ok(bytes) => event.field(name, hex_bytes(&bytes)),
        Err(error) => event.field(name, unavailable(&error)),
    }
}

fn supervisor_role(cmdline: &[u8]) -> Option<ProcessRole> {
    let fields = cmdline
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    fields
        .windows(2)
        .find(|pair| pair[0] == b"--role")
        .and_then(|pair| catalog::role(OsStr::from_bytes(pair[1])).ok())
}

fn unavailable(error: &io::Error) -> String {
    format!(
        "UNAVAILABLE:{:?}:{}:{}",
        error.kind(),
        error
            .raw_os_error()
            .map_or_else(|| "NONE".to_owned(), |value| value.to_string()),
        hex_bytes(error.to_string().as_bytes())
    )
}

unsafe extern "C" {
    fn prctl(
        option: c_int,
        argument2: c_ulong,
        argument3: c_ulong,
        argument4: c_ulong,
        argument5: c_ulong,
    ) -> c_int;
}
