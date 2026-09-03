use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::event::{hex_bytes, Event};

const PROC_FILES: &[(&str, &str)] = &[
    ("proc_comm_hex", "comm"),
    ("proc_status_hex", "status"),
    ("proc_cmdline_hex", "cmdline"),
    ("proc_cgroup_hex", "cgroup"),
    ("proc_limits_hex", "limits"),
    ("proc_sched_hex", "sched"),
    ("proc_oom_score_hex", "oom_score"),
    ("proc_oom_score_adj_hex", "oom_score_adj"),
];
const PROC_LINKS: &[(&str, &str)] = &[
    ("proc_exe_hex", "exe"),
    ("proc_cwd_hex", "cwd"),
    ("proc_root_hex", "root"),
    ("proc_ns_cgroup_hex", "ns/cgroup"),
    ("proc_ns_ipc_hex", "ns/ipc"),
    ("proc_ns_mnt_hex", "ns/mnt"),
    ("proc_ns_net_hex", "ns/net"),
    ("proc_ns_pid_hex", "ns/pid"),
    ("proc_ns_user_hex", "ns/user"),
    ("proc_ns_uts_hex", "ns/uts"),
];
const SAFE_ENVIRONMENT: &[&str] = &[
    "INI_FILE_NAME",
    "LINUXCNC_FORCE_REALTIME",
    "NMLFILE",
    "PYTHONDONTWRITEBYTECODE",
    "RTAPI_FIFO_PATH",
    "RTAPI_UID",
];
const HOST_FILES: &[(&str, &str)] = &[
    ("host_boot_id_hex", "/proc/sys/kernel/random/boot_id"),
    ("host_kernel_osrelease_hex", "/proc/sys/kernel/osrelease"),
    ("host_proc_version_hex", "/proc/version"),
    ("host_uptime_hex", "/proc/uptime"),
];

pub fn executable_event_fields(mut event: Event, program: &Path) -> Event {
    let canonical = fs::canonicalize(program);
    event = match canonical {
        Ok(path) => event
            .field("executable_identity_state", "captured")
            .encoded_path_field("executable_canonical_hex", &path),
        Err(error) => event
            .field("executable_identity_state", "canonicalize-failed")
            .field(
                "executable_identity_error_kind",
                format!("{:?}", error.kind()),
            )
            .field(
                "executable_identity_raw_os_error",
                optional_i32(error.raw_os_error()),
            )
            .field(
                "executable_identity_error_hex",
                hex_bytes(error.to_string().as_bytes()),
            ),
    };
    match fs::metadata(program) {
        Ok(metadata) => event
            .field("executable_device", metadata.dev())
            .field("executable_inode", metadata.ino())
            .field("executable_mode", metadata.mode())
            .field("executable_links", metadata.nlink())
            .field("executable_uid", metadata.uid())
            .field("executable_gid", metadata.gid())
            .field("executable_size", metadata.size())
            .field("executable_mtime_seconds", metadata.mtime())
            .field("executable_mtime_nanoseconds", metadata.mtime_nsec())
            .field("executable_ctime_seconds", metadata.ctime())
            .field("executable_ctime_nanoseconds", metadata.ctime_nsec()),
        Err(error) => event
            .field("executable_metadata_state", "unavailable")
            .field(
                "executable_metadata_error_kind",
                format!("{:?}", error.kind()),
            )
            .field(
                "executable_metadata_raw_os_error",
                optional_i32(error.raw_os_error()),
            )
            .field(
                "executable_metadata_error_hex",
                hex_bytes(error.to_string().as_bytes()),
            ),
    }
}

pub fn environment_event_fields(mut event: Event) -> Event {
    for name in SAFE_ENVIRONMENT {
        let field = match *name {
            "INI_FILE_NAME" => "env_ini_file_name_hex",
            "LINUXCNC_FORCE_REALTIME" => "env_linuxcnc_force_realtime_hex",
            "NMLFILE" => "env_nmlfile_hex",
            "PYTHONDONTWRITEBYTECODE" => "env_python_dont_write_bytecode_hex",
            "RTAPI_FIFO_PATH" => "env_rtapi_fifo_path_hex",
            "RTAPI_UID" => "env_rtapi_uid_hex",
            _ => unreachable!("safe environment catalog is exhaustive"),
        };
        event = match std::env::var_os(name) {
            Some(value) => event.field(field, hex_bytes(value.as_bytes())),
            None => event.field(field, "UNSET"),
        };
    }
    event
}

pub fn host_event_fields(mut event: Event) -> Event {
    for (field, path) in HOST_FILES {
        event = match fs::read(path) {
            Ok(bytes) => event.field(field, hex_bytes(&bytes)),
            Err(error) => event.field(field, unavailable(&error)),
        };
    }
    event
}

pub fn child_event_fields(mut event: Event, pid: u32) -> Event {
    let root = PathBuf::from(format!("/proc/{pid}"));
    let mut captured = 0_usize;
    let mut failed = 0_usize;
    match fs::read(root.join("stat")) {
        Ok(bytes) => {
            captured += 1;
            event = event.field("proc_stat_hex", hex_bytes(&bytes));
            event = match parse_stat(&bytes) {
                Ok(summary) => summary.event_fields(event),
                Err(error) => event
                    .field("proc_stat_parse_state", "invalid")
                    .field("proc_stat_parse_error", error),
            };
        }
        Err(error) => {
            failed += 1;
            event = event
                .field("proc_stat_hex", unavailable(&error))
                .field("proc_stat_parse_state", "unavailable");
        }
    }
    for (field, relative) in PROC_FILES {
        match fs::read(root.join(relative)) {
            Ok(bytes) => {
                captured += 1;
                event = event.field(field, hex_bytes(&bytes));
            }
            Err(error) => {
                failed += 1;
                event = event.field(field, unavailable(&error));
            }
        }
    }
    for (field, relative) in PROC_LINKS {
        match fs::read_link(root.join(relative)) {
            Ok(value) => {
                captured += 1;
                event = event.field(field, hex_bytes(value.as_os_str().as_bytes()));
            }
            Err(error) => {
                failed += 1;
                event = event.field(field, unavailable(&error));
            }
        }
    }
    event
        .field("process_snapshot_fields_captured", captured)
        .field("process_snapshot_fields_failed", failed)
        .field(
            "process_snapshot_state",
            if failed == 0 { "complete" } else { "partial" },
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StatSummary<'a> {
    reported_pid: &'a str,
    state: &'a str,
    parent_pid: &'a str,
    process_group_id: &'a str,
    session_id: &'a str,
    user_ticks: &'a str,
    system_ticks: &'a str,
    thread_count: &'a str,
    start_time_ticks: &'a str,
}

impl StatSummary<'_> {
    fn event_fields(self, event: Event) -> Event {
        event
            .field("proc_stat_parse_state", "captured")
            .field("proc_reported_pid", self.reported_pid)
            .field("proc_state", self.state)
            .field("proc_parent_pid", self.parent_pid)
            .field("proc_process_group_id", self.process_group_id)
            .field("proc_session_id", self.session_id)
            .field("proc_user_ticks", self.user_ticks)
            .field("proc_system_ticks", self.system_ticks)
            .field("proc_thread_count", self.thread_count)
            .field("proc_start_time_ticks", self.start_time_ticks)
    }
}

fn parse_stat(bytes: &[u8]) -> Result<StatSummary<'_>, &'static str> {
    let closing_parenthesis = bytes
        .iter()
        .rposition(|byte| *byte == b')')
        .ok_or("missing-command-closing-parenthesis")?;
    let command_opening = bytes[..closing_parenthesis]
        .windows(2)
        .position(|window| window == b" (")
        .ok_or("missing-command-opening-parenthesis")?;
    let reported_pid = std::str::from_utf8(&bytes[..command_opening])
        .map_err(|_| "pid-is-not-ascii")?
        .trim();
    let remainder = bytes
        .get(closing_parenthesis + 1..)
        .ok_or("missing-fields-after-command")?;
    let remainder = std::str::from_utf8(remainder).map_err(|_| "fields-are-not-ascii")?;
    let fields = remainder.split_ascii_whitespace().collect::<Vec<_>>();
    if fields.len() < 20 {
        return Err("fewer-than-22-proc-stat-fields");
    }
    Ok(StatSummary {
        reported_pid,
        state: fields[0],
        parent_pid: fields[1],
        process_group_id: fields[2],
        session_id: fields[3],
        user_ticks: fields[11],
        system_ticks: fields[12],
        thread_count: fields[17],
        start_time_ticks: fields[19],
    })
}

fn unavailable(error: &io::Error) -> String {
    format!(
        "UNAVAILABLE:{:?}:{}:{}",
        error.kind(),
        optional_i32(error.raw_os_error()),
        hex_bytes(error.to_string().as_bytes())
    )
}

fn optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "NONE".to_owned(), |value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_parent_and_timing_identity_from_linux_proc_stat() {
        let stat = b"42 (name with ) parenthesis) S 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27\n";
        let summary = parse_stat(stat).expect("parse representative proc stat");

        assert_eq!(summary.reported_pid, "42");
        assert_eq!(summary.state, "S");
        assert_eq!(summary.parent_pid, "7");
        assert_eq!(summary.process_group_id, "8");
        assert_eq!(summary.session_id, "9");
        assert_eq!(summary.user_ticks, "17");
        assert_eq!(summary.system_ticks, "18");
        assert_eq!(summary.thread_count, "23");
        assert_eq!(summary.start_time_ticks, "25");
    }
}
