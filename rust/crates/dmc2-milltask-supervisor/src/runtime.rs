use std::ffi::OsString;
use std::fmt;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, ExitStatus};
use std::time::{Instant, SystemTime, SystemTimeError, UNIX_EPOCH};

use crate::backtrace::{self, BacktraceEvidence};
use crate::cli::{CliError, Invocation};
use crate::event::{encode_arguments, hex_bytes, signal_name, Event};
use crate::journal::{Journal, JournalError};

pub const TRACKING_FAILURE_EXIT_CODE: u8 = 125;

pub fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<u8, SupervisorError> {
    let invocation = Invocation::parse(arguments).map_err(SupervisorError::Cli)?;
    supervise(invocation)
}

fn supervise(invocation: Invocation) -> Result<u8, SupervisorError> {
    let supervisor_pid = std::process::id();
    let mut journal = Journal::open(&invocation.journal).map_err(SupervisorError::Journal)?;
    let supervisor_started_ns = unix_ns().map_err(SupervisorError::Clock)?;
    let recovered_partial_record = journal.recovered_partial_record();
    let journal_path = journal.path().to_path_buf();
    journal
        .append(
            &Event::new("supervisor-started", supervisor_started_ns, supervisor_pid)
                .field("recovered_partial_record", recovered_partial_record)
                .encoded_path_field("journal_path_hex", &journal_path),
        )
        .map_err(SupervisorError::Journal)?;

    let launched_at_wall = SystemTime::now();
    let launched_at_monotonic = Instant::now();
    let mut child = match Command::new(&invocation.program)
        .args(&invocation.arguments)
        .spawn()
    {
        Ok(child) => child,
        Err(source) => {
            let event = Event::new("spawn-failed", unix_ns_or_zero(), supervisor_pid)
                .encoded_os_field("program_hex", &invocation.program)
                .field("error_kind", format!("{:?}", source.kind()))
                .field("raw_os_error", optional_i32(source.raw_os_error()));
            let _ = journal.append(&event);
            return Err(SupervisorError::Spawn {
                program: invocation.program,
                source,
            });
        }
    };
    let child_pid = child.id();
    let start_record_error = journal
        .append(
            &Event::new("milltask-started", unix_ns_or_zero(), supervisor_pid)
                .field("milltask_pid", child_pid)
                .encoded_os_field("program_hex", &invocation.program)
                .field("argc", invocation.arguments.len())
                .field("argv_hex", encode_arguments(&invocation.arguments)),
        )
        .err();
    if let Some(error) = &start_record_error {
        eprintln!(
            "dmc2-milltask-supervisor: milltask PID {child_pid} is running, but its start record could not be persisted: {error}"
        );
    }

    let status = child
        .wait()
        .map_err(|source| SupervisorError::Wait { child_pid, source })?;
    let exit_ns = unix_ns().map_err(SupervisorError::Clock)?;
    let elapsed_ns = launched_at_monotonic.elapsed().as_nanos();
    let backtrace = backtrace::capture(journal.path(), child_pid, launched_at_wall, exit_ns);
    let event = termination_event(
        supervisor_pid,
        child_pid,
        exit_ns,
        elapsed_ns,
        status,
        &backtrace,
    );
    let terminal_record = journal.append(&event);
    if let Some(error) = start_record_error {
        if let Err(terminal_error) = terminal_record {
            eprintln!(
                "dmc2-milltask-supervisor: milltask PID {child_pid} terminated, but its terminal record also could not be persisted: {terminal_error}"
            );
        }
        return Err(SupervisorError::JournalAfterSpawn(error));
    }
    terminal_record.map_err(SupervisorError::JournalAfterTermination)?;

    Ok(supervisor_exit_code(status))
}

fn termination_event(
    supervisor_pid: u32,
    child_pid: u32,
    exit_ns: u128,
    elapsed_ns: u128,
    status: ExitStatus,
    backtrace: &BacktraceEvidence,
) -> Event {
    let mut event = Event::new("milltask-terminated", exit_ns, supervisor_pid)
        .field("milltask_pid", child_pid)
        .field("elapsed_ns", elapsed_ns)
        .field("raw_wait_status", status.into_raw())
        .field("exit_code", optional_i32(status.code()))
        .field("signal", optional_i32(status.signal()))
        .field(
            "signal_name",
            status.signal().map(signal_name).unwrap_or("NONE"),
        )
        .field("core_dumped", status.core_dumped());

    event = match backtrace {
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

    let outcome = match (status.signal(), status.code(), backtrace.reported_signal()) {
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
    Journal(JournalError),
    JournalAfterSpawn(JournalError),
    JournalAfterTermination(JournalError),
    Clock(SystemTimeError),
    Spawn {
        program: OsString,
        source: io::Error,
    },
    Wait {
        child_pid: u32,
        source: io::Error,
    },
}

impl fmt::Display for SupervisorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(error) => write!(formatter, "invalid invocation: {error}"),
            Self::Journal(error) => write!(formatter, "lifecycle journal unavailable: {error}"),
            Self::JournalAfterSpawn(error) => write!(
                formatter,
                "milltask was spawned but its start record could not be persisted: {error}"
            ),
            Self::JournalAfterTermination(error) => write!(
                formatter,
                "milltask terminated but its terminal record could not be persisted: {error}"
            ),
            Self::Clock(error) => write!(formatter, "system clock predates Unix epoch: {error}"),
            Self::Spawn { program, source } => {
                write!(formatter, "could not spawn {program:?}: {source}")
            }
            Self::Wait { child_pid, source } => {
                write!(
                    formatter,
                    "could not wait for milltask PID {child_pid}: {source}"
                )
            }
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
