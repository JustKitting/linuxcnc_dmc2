use std::ffi::OsString;
use std::fmt;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::time::{Instant, SystemTime, SystemTimeError, UNIX_EPOCH};

use crate::backtrace::{self, BacktraceEvidence};
use crate::catalog::{BacktraceKind, Ownership, ProcessRole};
use crate::cli::{CliError, Invocation};
use crate::event::{encode_arguments, hex_bytes, signal_name, Event};
use crate::journal::{Journal, JournalError};
use crate::process;
use crate::wait::{self, WaitEvidence};

pub const TRACKING_FAILURE_EXIT_CODE: u8 = 125;

pub fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<u8, SupervisorError> {
    let invocation = Invocation::parse(arguments).map_err(SupervisorError::Cli)?;
    if invocation.role.ownership() != Ownership::DirectChild {
        return Err(SupervisorError::UnsupportedOwnership {
            role: invocation.role,
            ownership: invocation.role.ownership(),
        });
    }
    supervise(invocation)
}

fn supervise(invocation: Invocation) -> Result<u8, SupervisorError> {
    let supervisor_pid = std::process::id();
    let mut journal = Journal::open(&invocation.journal).map_err(SupervisorError::Journal)?;
    let supervisor_started_ns = unix_ns().map_err(SupervisorError::Clock)?;
    let recovered_partial_record = journal.recovered_partial_record();
    let journal_path = journal.path().to_path_buf();
    let supervisor_event = base_event(
        "supervisor-started",
        supervisor_started_ns,
        supervisor_pid,
        invocation.role,
    )
    .field("recovered_partial_record", recovered_partial_record)
    .encoded_path_field("journal_path_hex", &journal_path)
    .encoded_os_field("program_hex", &invocation.program)
    .field("argc", invocation.arguments.len())
    .field("argv_hex", encode_arguments(&invocation.arguments));
    let supervisor_event = process::environment_event_fields(supervisor_event);
    let supervisor_event = process::host_event_fields(supervisor_event);
    let supervisor_event =
        process::executable_event_fields(supervisor_event, Path::new(&invocation.program));
    let supervisor_event = process::child_event_fields(supervisor_event, supervisor_pid);
    journal
        .append(&supervisor_event)
        .map_err(SupervisorError::Journal)?;

    let launched_at_wall = SystemTime::now();
    let launched_at_monotonic = Instant::now();
    let mut child = match Command::new(&invocation.program)
        .args(&invocation.arguments)
        .spawn()
    {
        Ok(child) => child,
        Err(source) => {
            let event = base_event(
                "spawn-failed",
                unix_ns_or_zero(),
                supervisor_pid,
                invocation.role,
            )
            .encoded_os_field("program_hex", &invocation.program)
            .field("error_kind", format!("{:?}", source.kind()))
            .field("raw_os_error", optional_i32(source.raw_os_error()))
            .field("error_hex", hex_bytes(source.to_string().as_bytes()));
            return match journal.append(&event) {
                Ok(()) => Err(SupervisorError::Spawn {
                    role: invocation.role,
                    program: invocation.program,
                    source,
                }),
                Err(journal) => Err(SupervisorError::SpawnAndJournal {
                    role: invocation.role,
                    program: invocation.program,
                    spawn: source,
                    journal,
                }),
            };
        }
    };
    let child_pid = child.id();
    let start_event = base_event(
        "process-started",
        unix_ns_or_zero(),
        supervisor_pid,
        invocation.role,
    )
    .field("child_pid", child_pid)
    .encoded_os_field("program_hex", &invocation.program)
    .field("argc", invocation.arguments.len())
    .field("argv_hex", encode_arguments(&invocation.arguments));
    let start_event = process::child_event_fields(start_event, child_pid);
    let start_record_error = journal.append(&start_event).err();
    if let Some(error) = &start_record_error {
        eprintln!(
            "dmc2-process-supervisor: role={} child_pid={child_pid} start_record=failed error={error} fallback_event={}",
            invocation.role.name(),
            start_event.render()
        );
    }

    let evidence = match wait::wait(&mut child) {
        Ok(evidence) => evidence,
        Err(source) => {
            let event = base_event(
                "wait-failed",
                unix_ns_or_zero(),
                supervisor_pid,
                invocation.role,
            )
            .field("child_pid", child_pid)
            .field("error_kind", format!("{:?}", source.kind()))
            .field("raw_os_error", optional_i32(source.raw_os_error()))
            .field("error_hex", hex_bytes(source.to_string().as_bytes()));
            let terminal_record = journal.append(&event);
            if let Some(error) = start_record_error {
                if let Err(terminal_error) = terminal_record {
                    eprintln!(
                        "dmc2-process-supervisor: role={} child_pid={child_pid} wait=failed start_record=failed terminal_record=failed start_error={error} terminal_error={terminal_error} fallback_event={}",
                        invocation.role.name(),
                        event.render()
                    );
                }
                return Err(SupervisorError::JournalAfterSpawn(error));
            }
            terminal_record.map_err(SupervisorError::JournalAfterTermination)?;
            return Err(SupervisorError::Wait {
                role: invocation.role,
                child_pid,
                source,
            });
        }
    };
    let exit_ns = unix_ns_or_zero();
    let elapsed_ns = launched_at_monotonic.elapsed().as_nanos();
    let backtrace = match invocation.role.backtrace() {
        BacktraceKind::None => BacktraceEvidence::NotApplicable,
        BacktraceKind::LinuxCncTask => {
            backtrace::capture(journal.path(), child_pid, launched_at_wall, exit_ns)
        }
    };
    let event = termination_event(
        supervisor_pid,
        invocation.role,
        exit_ns,
        elapsed_ns,
        evidence,
        &backtrace,
    );
    let terminal_record = journal.append(&event);
    if let Some(error) = start_record_error {
        if let Err(terminal_error) = terminal_record {
            eprintln!(
                "dmc2-process-supervisor: role={} child_pid={child_pid} terminal_record=failed start_error={error} terminal_error={terminal_error} fallback_event={}",
                invocation.role.name(),
                event.render()
            );
        }
        return Err(SupervisorError::JournalAfterSpawn(error));
    }
    if let Err(error) = terminal_record {
        eprintln!(
            "dmc2-process-supervisor: role={} child_pid={child_pid} terminal_record=failed error={error} fallback_event={}",
            invocation.role.name(),
            event.render()
        );
        return Err(SupervisorError::JournalAfterTermination(error));
    }

    Ok(supervisor_exit_code(evidence.status))
}

fn base_event(kind: &'static str, unix_ns: u128, supervisor_pid: u32, role: ProcessRole) -> Event {
    Event::new(kind, unix_ns, supervisor_pid)
        .field("role", role.name())
        .field("launch_site", role.launch_site())
        .field("ownership", role.ownership().name())
        .field("criticality", role.criticality().name())
        .field("backtrace_contract", role.backtrace().name())
        .field("argument_placement", role.argument_placement().name())
}

fn termination_event(
    supervisor_pid: u32,
    role: ProcessRole,
    exit_ns: u128,
    elapsed_ns: u128,
    evidence: WaitEvidence,
    backtrace: &BacktraceEvidence,
) -> Event {
    let event = base_event("process-terminated", exit_ns, supervisor_pid, role)
        .field("elapsed_ns", elapsed_ns);
    let mut event = evidence.event_fields(event);

    event = match backtrace {
        BacktraceEvidence::NotApplicable => event.field("backtrace_state", "not-applicable"),
        BacktraceEvidence::Absent { source } => event
            .field("backtrace_state", "absent")
            .encoded_path_field("backtrace_source_hex", source),
        BacktraceEvidence::Captured {
            source,
            durable_copy,
            reported_signal,
        } => event
            .field("backtrace_state", "captured")
            .encoded_path_field("backtrace_source_hex", source)
            .encoded_path_field("backtrace_copy_hex", durable_copy)
            .field("backtrace_signal", optional_i32(*reported_signal))
            .field(
                "backtrace_signal_name",
                reported_signal.map(signal_name).unwrap_or("NONE"),
            ),
        BacktraceEvidence::Rejected { source, reason } => event
            .field("backtrace_state", "rejected")
            .encoded_path_field("backtrace_source_hex", source)
            .field("backtrace_rejection", reason),
        BacktraceEvidence::CaptureFailed {
            source,
            operation,
            error,
        } => event
            .field("backtrace_state", "capture-failed")
            .encoded_path_field("backtrace_source_hex", source)
            .field("backtrace_operation", operation)
            .field("backtrace_error_kind", format!("{:?}", error.kind()))
            .field("backtrace_raw_os_error", optional_i32(error.raw_os_error()))
            .field(
                "backtrace_error_hex",
                hex_bytes(error.to_string().as_bytes()),
            ),
    };

    let outcome = match (
        evidence.status.signal(),
        evidence.status.code(),
        backtrace.reported_signal(),
    ) {
        (_, _, Some(8 | 11)) => "linuxcnc-handled-fatal-signal",
        (Some(_), _, _) => "kernel-signal-termination",
        (None, Some(0), _) => "zero-exit",
        (None, Some(_), _) => "nonzero-exit",
        _ => "unknown-wait-status",
    };
    event.field("outcome", outcome)
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

#[derive(Debug)]
pub enum SupervisorError {
    Cli(CliError),
    UnsupportedOwnership {
        role: ProcessRole,
        ownership: Ownership,
    },
    Journal(JournalError),
    JournalAfterSpawn(JournalError),
    JournalAfterTermination(JournalError),
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
    Wait {
        role: ProcessRole,
        child_pid: u32,
        source: io::Error,
    },
}

impl fmt::Display for SupervisorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(error) => write!(formatter, "invalid invocation: {error}"),
            Self::UnsupportedOwnership { role, ownership } => write!(
                formatter,
                "role {} has ownership contract {}, but this binary only owns direct children",
                role.name(),
                ownership.name()
            ),
            Self::Journal(error) => write!(formatter, "lifecycle journal unavailable: {error}"),
            Self::JournalAfterSpawn(error) => write!(
                formatter,
                "a supervised process was spawned but its start record could not be persisted: {error}"
            ),
            Self::JournalAfterTermination(error) => write!(
                formatter,
                "a supervised process terminated but its terminal record could not be persisted: {error}"
            ),
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
            Self::Wait {
                role,
                child_pid,
                source,
            } => write!(
                formatter,
                "could not wait for role {} child PID {child_pid}: {source}",
                role.name()
            ),
        }
    }
}

impl std::error::Error for SupervisorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cli(error) => Some(error),
            Self::Journal(error)
            | Self::JournalAfterSpawn(error)
            | Self::JournalAfterTermination(error) => Some(error),
            Self::Clock(error) => Some(error),
            Self::Spawn { source, .. } | Self::Wait { source, .. } => Some(source),
            Self::SpawnAndJournal { spawn, .. } => Some(spawn),
            Self::UnsupportedOwnership { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirrors_normal_child_status() {
        assert_eq!(supervisor_exit_code(ExitStatus::from_raw(23 << 8)), 23);
    }

    #[test]
    fn represents_signal_status_using_the_shell_convention() {
        assert_eq!(supervisor_exit_code(ExitStatus::from_raw(6)), 134);
    }
}
