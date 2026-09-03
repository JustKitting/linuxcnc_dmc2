use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::os::raw::{c_int, c_ulong};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant, SystemTime, SystemTimeError, UNIX_EPOCH};

use crate::catalog::{self, Ownership, ProcessRole};
use crate::cli::{CliError, Invocation};
use crate::event::{encode_arguments, hex_bytes, Event};
use crate::journal::{Journal, JournalError};
use crate::process;
use crate::runtime::TRACKING_FAILURE_EXIT_CODE;
use crate::wait::{self, AnyWait, WaitEvidence};

const OBSERVATION_PERIOD: Duration = Duration::from_millis(5);

pub fn run_session(arguments: impl IntoIterator<Item = OsString>) -> Result<u8, SessionError> {
    let invocation = Invocation::parse(arguments).map_err(SessionError::Cli)?;
    if invocation.role.ownership() != Ownership::SessionRoot {
        return Err(SessionError::UnsupportedOwnership {
            role: invocation.role,
            ownership: invocation.role.ownership(),
        });
    }
    supervise(invocation)
}

fn supervise(invocation: Invocation) -> Result<u8, SessionError> {
    let supervisor_pid = std::process::id();
    let mut journal = Journal::open(&invocation.journal).map_err(SessionError::Journal)?;
    enable_child_subreaper().map_err(SessionError::EnableSubreaper)?;
    verify_child_subreaper().map_err(SessionError::VerifySubreaper)?;

    let session_started_ns = unix_ns().map_err(SessionError::Clock)?;
    let event = session_event(
        "session-supervisor-started",
        session_started_ns,
        supervisor_pid,
    )
    .field("role", invocation.role.name())
    .field("ownership", invocation.role.ownership().name())
    .field("observation_period_ns", OBSERVATION_PERIOD.as_nanos())
    .field(
        "recovered_partial_record",
        journal.recovered_partial_record(),
    )
    .encoded_path_field("journal_path_hex", journal.path())
    .encoded_os_field("program_hex", &invocation.program)
    .field("argc", invocation.arguments.len())
    .field("argv_hex", encode_arguments(&invocation.arguments));
    let event = process::environment_event_fields(event);
    let event = process::host_event_fields(event);
    let event = process::executable_event_fields(event, Path::new(&invocation.program));
    let event = process::child_event_fields(event, supervisor_pid);
    journal.append(&event).map_err(SessionError::Journal)?;

    let linuxcnc = match Command::new(&invocation.program)
        .args(&invocation.arguments)
        .spawn()
    {
        Ok(child) => child,
        Err(source) => {
            let event = session_event("session-spawn-failed", unix_ns_or_zero(), supervisor_pid)
                .field("role", invocation.role.name())
                .encoded_os_field("program_hex", &invocation.program)
                .field("error_kind", format!("{:?}", source.kind()))
                .field("raw_os_error", optional_i32(source.raw_os_error()))
                .field("error_hex", hex_bytes(source.to_string().as_bytes()));
            return match journal.append(&event) {
                Ok(()) => Err(SessionError::Spawn {
                    role: invocation.role,
                    program: invocation.program,
                    source,
                }),
                Err(journal) => Err(SessionError::SpawnAndJournal {
                    role: invocation.role,
                    program: invocation.program,
                    spawn: source,
                    journal,
                }),
            };
        }
    };
    let linuxcnc_pid = linuxcnc.id();
    let observed_at = Instant::now();
    let mut children = BTreeMap::new();
    let root = ObservedChild {
        role: invocation.role.name().to_owned(),
        catalog_role: Some(invocation.role),
        relation: "direct-session-child",
        layer: "session-root",
        identity_source: "launcher-role",
        first_observed: observed_at,
    };
    let event = child_started_event(supervisor_pid, linuxcnc_pid, &root);
    let mut retained_journal_errors = Vec::new();
    append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
    children.insert(linuxcnc_pid, root);

    let mut linuxcnc_status = None;
    let mut children_probe_error_reported = false;
    let mut wait_error_reported = false;
    loop {
        match direct_children(supervisor_pid) {
            Ok(pids) => {
                if children_probe_error_reported {
                    let event = session_event(
                        "session-children-probe-restored",
                        unix_ns_or_zero(),
                        supervisor_pid,
                    );
                    append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                }
                children_probe_error_reported = false;
                for pid in pids {
                    if children.contains_key(&pid) {
                        continue;
                    }
                    let child = classify_child(pid);
                    let event = child_started_event(supervisor_pid, pid, &child);
                    append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                    children.insert(pid, child);
                }
            }
            Err(error) if !children_probe_error_reported => {
                children_probe_error_reported = true;
                let event = session_event(
                    "session-children-probe-failed",
                    unix_ns_or_zero(),
                    supervisor_pid,
                )
                .field("error_kind", format!("{:?}", error.kind()))
                .field("raw_os_error", optional_i32(error.raw_os_error()))
                .field("error_hex", hex_bytes(error.to_string().as_bytes()));
                append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
            }
            Err(_) => {}
        }

        let mut no_children = false;
        loop {
            let wait_result = wait::wait_any_nonblocking();
            if wait_error_reported && wait_result.is_ok() {
                let event =
                    session_event("session-wait-restored", unix_ns_or_zero(), supervisor_pid);
                append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                wait_error_reported = false;
            }
            match wait_result {
                Ok(AnyWait::Exited(evidence)) => {
                    let child = children.remove(&evidence.pid);
                    let event = child_terminated_event(supervisor_pid, child.as_ref(), evidence);
                    append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                    if evidence.pid == linuxcnc_pid {
                        linuxcnc_status = Some(evidence.status);
                    }
                }
                Ok(AnyWait::Running) => break,
                Ok(AnyWait::NoChildren) => {
                    no_children = true;
                    break;
                }
                Err(error) if !wait_error_reported => {
                    wait_error_reported = true;
                    let event =
                        session_event("session-wait-failed", unix_ns_or_zero(), supervisor_pid)
                            .field("error_kind", format!("{:?}", error.kind()))
                            .field("raw_os_error", optional_i32(error.raw_os_error()))
                            .field("error_hex", hex_bytes(error.to_string().as_bytes()));
                    append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                    break;
                }
                Err(_) => break,
            }
        }

        if no_children {
            let Some(status) = linuxcnc_status else {
                let event = session_event(
                    "session-invariant-failed",
                    unix_ns_or_zero(),
                    supervisor_pid,
                )
                .field("identity", "LINUXCNC_ROOT_WAIT_STATUS_MISSING")
                .field("linuxcnc_pid", linuxcnc_pid)
                .field("remaining_children", children.len());
                append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                return Err(SessionError::LinuxCncStatusMissing { linuxcnc_pid });
            };
            let event = session_event(
                "session-supervisor-terminated",
                unix_ns_or_zero(),
                supervisor_pid,
            )
            .field("linuxcnc_pid", linuxcnc_pid)
            .field("linuxcnc_raw_wait_status", status.into_raw())
            .field("linuxcnc_exit_code", optional_i32(status.code()))
            .field("linuxcnc_signal", optional_i32(status.signal()))
            .field("remaining_children", children.len());
            append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
            if !retained_journal_errors.is_empty() {
                let first = retained_journal_errors.remove(0);
                return Err(SessionError::JournalAfterSpawn {
                    first,
                    additional_failures: retained_journal_errors.len(),
                });
            }
            return Ok(supervisor_exit_code(status));
        }

        thread::sleep(OBSERVATION_PERIOD);
    }
}

fn child_started_event(supervisor_pid: u32, pid: u32, child: &ObservedChild) -> Event {
    let event = session_event("session-child-observed", unix_ns_or_zero(), supervisor_pid)
        .field("role", &child.role)
        .field("relation", child.relation)
        .field("layer", child.layer)
        .field("identity_source", child.identity_source)
        .field("child_pid", pid);
    let event = catalog_event_fields(event, child.catalog_role);
    process::child_event_fields(event, pid)
}

fn child_terminated_event(
    supervisor_pid: u32,
    child: Option<&ObservedChild>,
    evidence: WaitEvidence,
) -> Event {
    let (role, relation, observed_ns, observation_state) = match child {
        Some(child) => (
            child.role.as_str(),
            child.relation,
            child.first_observed.elapsed().as_nanos(),
            "start-observed",
        ),
        None => (
            "unknown",
            "unobserved-session-descendant",
            0,
            "terminal-only",
        ),
    };
    let event = session_event(
        "session-child-terminated",
        unix_ns_or_zero(),
        supervisor_pid,
    )
    .field("role", role)
    .field("relation", relation)
    .field("layer", child.map_or("unobserved", |child| child.layer))
    .field(
        "identity_source",
        child.map_or("unavailable-after-reap", |child| child.identity_source),
    )
    .field("observation_state", observation_state)
    .field("elapsed_since_observed_ns", observed_ns);
    let event = catalog_event_fields(event, child.and_then(|child| child.catalog_role));
    let outcome = evidence.kernel_outcome();
    evidence.event_fields(event).field("outcome", outcome)
}

fn classify_child(pid: u32) -> ObservedChild {
    let root = PathBuf::from(format!("/proc/{pid}"));
    let executable = fs::read_link(root.join("exe"));
    let cmdline = fs::read(root.join("cmdline")).unwrap_or_default();
    let comm = fs::read(root.join("comm")).ok();
    let basename = executable
        .as_ref()
        .ok()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str());
    if basename == Some("dmc2-process-supervisor") {
        return match process_supervisor_role(&cmdline).and_then(|name| {
            catalog::role(OsStr::new(&name))
                .ok()
                .map(|role| (name, role))
        }) {
            Some((name, role)) => ObservedChild::classified(
                name,
                role,
                "direct-process-owner",
                "supervisor-command-line-role",
            ),
            None => ObservedChild::unknown(
                "process-supervisor:unknown",
                "direct-process-owner",
                "unmatched",
            ),
        };
    }
    match catalog::identify_process(executable.as_deref().ok(), &cmdline, comm.as_deref()) {
        Ok(Some((role, source))) => {
            ObservedChild::classified(role.name(), role, "catalogued-workload", source.name())
        }
        Ok(None) | Err(_) => match basename {
            Some(name) => ObservedChild::unknown(
                format!("uncatalogued:{name}"),
                "uncatalogued-descendant",
                "proc-executable-basename-only",
            ),
            None => ObservedChild::unknown("unknown", "unidentified-descendant", "unmatched"),
        },
    }
}

fn catalog_event_fields(event: Event, role: Option<ProcessRole>) -> Event {
    match role {
        Some(role) => event
            .field("catalog_state", "matched")
            .field("catalog_program", role.program())
            .field("catalog_launch_site", role.launch_site())
            .field("catalog_ownership", role.ownership().name())
            .field("catalog_criticality", role.criticality().name())
            .field("catalog_backtrace", role.backtrace().name())
            .field(
                "catalog_argument_placement",
                role.argument_placement().name(),
            ),
        None => event.field("catalog_state", "unmatched"),
    }
}

fn process_supervisor_role(cmdline: &[u8]) -> Option<String> {
    let fields = cmdline
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    fields
        .windows(2)
        .find(|pair| pair[0] == b"--role")
        .map(|pair| String::from_utf8_lossy(pair[1]).into_owned())
}

fn direct_children(supervisor_pid: u32) -> io::Result<Vec<u32>> {
    let path = format!("/proc/self/task/{supervisor_pid}/children");
    let contents = fs::read_to_string(path)?;
    contents
        .split_ascii_whitespace()
        .map(|field| {
            field.parse::<u32>().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid child PID {field:?}: {error}"),
                )
            })
        })
        .collect()
}

fn enable_child_subreaper() -> io::Result<()> {
    const PR_SET_CHILD_SUBREAPER: c_int = 36;
    // SAFETY: PR_SET_CHILD_SUBREAPER consumes an integer flag and does not
    // dereference any argument or retain process memory.
    if unsafe { prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn verify_child_subreaper() -> io::Result<()> {
    const PR_GET_CHILD_SUBREAPER: c_int = 37;
    let mut enabled = 0_i32;
    // SAFETY: PR_GET_CHILD_SUBREAPER writes one c_int through the valid local
    // pointer and does not retain it.
    let result = unsafe {
        prctl(
            PR_GET_CHILD_SUBREAPER,
            (&mut enabled as *mut c_int) as c_ulong,
            0,
            0,
            0,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if enabled == 1 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "PR_GET_CHILD_SUBREAPER returned {enabled}"
        )))
    }
}

fn session_event(kind: &'static str, unix_ns: u128, supervisor_pid: u32) -> Event {
    Event::new(kind, unix_ns, supervisor_pid).field("tracker", "session-subreaper")
}

fn append_after_spawn(journal: &mut Journal, event: &Event, failures: &mut Vec<JournalError>) {
    if let Err(error) = journal.append(event) {
        eprintln!(
            "dmc2-session-supervisor: lifecycle_journal_append=failed error={error} fallback_event={}",
            event.render()
        );
        failures.push(error);
    }
}

fn first_source(error: &SessionError) -> Option<&(dyn std::error::Error + 'static)> {
    match error {
        SessionError::Cli(error) => Some(error),
        SessionError::Journal(error) => Some(error),
        SessionError::JournalAfterSpawn { first, .. } => Some(first),
        SessionError::EnableSubreaper(error)
        | SessionError::VerifySubreaper(error)
        | SessionError::Spawn { source: error, .. }
        | SessionError::SpawnAndJournal { spawn: error, .. } => Some(error),
        SessionError::UnsupportedOwnership { .. } | SessionError::LinuxCncStatusMissing { .. } => {
            None
        }
        SessionError::Clock(error) => Some(error),
    }
}

fn supervisor_exit_code(status: ExitStatus) -> u8 {
    if let Some(code) = status.code() {
        return u8::try_from(code).unwrap_or(TRACKING_FAILURE_EXIT_CODE);
    }
    status
        .signal()
        .and_then(|signal| u8::try_from(128_i32.saturating_add(signal)).ok())
        .unwrap_or(TRACKING_FAILURE_EXIT_CODE)
}

fn optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "NONE".to_owned(), |value| value.to_string())
}

fn unix_ns() -> Result<u128, SystemTimeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
}

fn unix_ns_or_zero() -> u128 {
    unix_ns().unwrap_or(0)
}

struct ObservedChild {
    role: String,
    catalog_role: Option<ProcessRole>,
    relation: &'static str,
    layer: &'static str,
    identity_source: &'static str,
    first_observed: Instant,
}

impl ObservedChild {
    fn classified(
        name: impl Into<String>,
        role: ProcessRole,
        layer: &'static str,
        identity_source: &'static str,
    ) -> Self {
        Self {
            role: name.into(),
            catalog_role: Some(role),
            relation: "adopted-session-descendant",
            layer,
            identity_source,
            first_observed: Instant::now(),
        }
    }

    fn unknown(
        name: impl Into<String>,
        layer: &'static str,
        identity_source: &'static str,
    ) -> Self {
        Self {
            role: name.into(),
            catalog_role: None,
            relation: "adopted-session-descendant",
            layer,
            identity_source,
            first_observed: Instant::now(),
        }
    }
}

#[derive(Debug)]
pub enum SessionError {
    Cli(CliError),
    UnsupportedOwnership {
        role: ProcessRole,
        ownership: Ownership,
    },
    Journal(JournalError),
    JournalAfterSpawn {
        first: JournalError,
        additional_failures: usize,
    },
    EnableSubreaper(io::Error),
    VerifySubreaper(io::Error),
    Clock(SystemTimeError),
    Spawn {
        role: ProcessRole,
        program: OsString,
        source: io::Error,
    },
    SpawnAndJournal {
        role: ProcessRole,
        program: OsString,
        spawn: io::Error,
        journal: JournalError,
    },
    LinuxCncStatusMissing {
        linuxcnc_pid: u32,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(error) => write!(formatter, "invalid invocation: {error}"),
            Self::UnsupportedOwnership { role, ownership } => write!(
                formatter,
                "role {} has ownership contract {}, not session-root",
                role.name(),
                ownership.name()
            ),
            Self::Journal(error) => write!(formatter, "lifecycle journal unavailable: {error}"),
            Self::JournalAfterSpawn {
                first,
                additional_failures,
            } => write!(
                formatter,
                "the session ran but {} lifecycle record(s) could not be persisted; first error: {first}",
                additional_failures + 1
            ),
            Self::EnableSubreaper(error) => {
                write!(
                    formatter,
                    "could not enable Linux child-subreaper ownership: {error}"
                )
            }
            Self::VerifySubreaper(error) => {
                write!(
                    formatter,
                    "could not verify Linux child-subreaper ownership: {error}"
                )
            }
            Self::Clock(error) => write!(formatter, "system clock predates Unix epoch: {error}"),
            Self::Spawn {
                role,
                program,
                source,
            } => write!(
                formatter,
                "could not spawn role {} program {program:?}: {source}",
                role.name()
            ),
            Self::SpawnAndJournal {
                role,
                program,
                spawn,
                journal,
            } => write!(
                formatter,
                "could not spawn role {} program {program:?}: {spawn}; the spawn-failure lifecycle record also failed: {journal}",
                role.name()
            ),
            Self::LinuxCncStatusMissing { linuxcnc_pid } => write!(
                formatter,
                "no children remain but LinuxCNC PID {linuxcnc_pid} had no captured wait status"
            ),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        first_source(self)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_process_supervisor_role_from_nul_separated_cmdline() {
        assert_eq!(
            process_supervisor_role(b"/bin/supervisor\0--role\0milltask\0--journal\0x\0"),
            Some("milltask".to_owned())
        );
    }

    #[test]
    fn reports_no_role_when_option_is_absent() {
        assert_eq!(
            process_supervisor_role(b"/bin/supervisor\0--journal\0x\0"),
            None
        );
    }
}
