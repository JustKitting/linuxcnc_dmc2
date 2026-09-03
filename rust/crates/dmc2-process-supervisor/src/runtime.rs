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
use crate::core_artifact::{self, CoreArtifactEvidence, WorkingDirectoryEvidence};
use crate::event::{encode_arguments, hex_bytes, Event};
use crate::journal::{Journal, JournalError};
use crate::limits::CoreDumpPlan;
use crate::process;
use crate::wait::{self, TerminalObservation, WaitEvidence};

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
    if let Err(source) = process::set_process_owner_identity(invocation.role) {
        let event = base_event(
            "owner-identity-failed",
            unix_ns_or_zero(),
            supervisor_pid,
            invocation.role,
        )
        .field("error_kind", format!("{:?}", source.kind()))
        .field("raw_os_error", optional_i32(source.raw_os_error()))
        .field("error_hex", hex_bytes(source.to_string().as_bytes()));
        return match journal.append(&event) {
            Ok(()) => Err(SupervisorError::OwnerIdentity {
                role: invocation.role,
                source,
            }),
            Err(journal) => Err(SupervisorError::OwnerIdentityAndJournal {
                role: invocation.role,
                identity: source,
                journal,
            }),
        };
    }
    let core_dump_plan = match CoreDumpPlan::capture(invocation.role.core_dump_policy()) {
        Ok(plan) => plan,
        Err(source) => {
            let event = base_event(
                "core-limit-plan-failed",
                unix_ns_or_zero(),
                supervisor_pid,
                invocation.role,
            )
            .field("error_kind", format!("{:?}", source.kind()))
            .field("raw_os_error", optional_i32(source.raw_os_error()))
            .field("error_hex", hex_bytes(source.to_string().as_bytes()));
            return match journal.append(&event) {
                Ok(()) => Err(SupervisorError::CoreDumpLimit {
                    role: invocation.role,
                    source,
                }),
                Err(journal) => Err(SupervisorError::CoreDumpLimitAndJournal {
                    role: invocation.role,
                    limit: source,
                    journal,
                }),
            };
        }
    };
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
    let supervisor_event = core_dump_plan.event_fields(supervisor_event);
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
    let fallback_cwd = WorkingDirectoryEvidence::for_supervisor();
    let mut command = Command::new(&invocation.program);
    command.args(&invocation.arguments);
    core_dump_plan.configure(&mut command);
    let mut child = match command.spawn() {
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
    let child_cwd = WorkingDirectoryEvidence::for_process(child_pid);
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
    let mut retained_journal_errors = Vec::new();
    append_after_spawn(
        &mut journal,
        &start_event,
        invocation.role,
        child_pid,
        &mut retained_journal_errors,
    );

    let observation = match wait::observe(&child) {
        Ok(observation) => observation,
        Err(source) => {
            let event = base_event(
                "terminal-observe-failed",
                unix_ns_or_zero(),
                supervisor_pid,
                invocation.role,
            )
            .field("child_pid", child_pid)
            .field("error_kind", format!("{:?}", source.kind()))
            .field("raw_os_error", optional_i32(source.raw_os_error()))
            .field("error_hex", hex_bytes(source.to_string().as_bytes()));
            append_after_spawn(
                &mut journal,
                &event,
                invocation.role,
                child_pid,
                &mut retained_journal_errors,
            );
            return Err(SupervisorError::Observe {
                role: invocation.role,
                child_pid,
                source,
            });
        }
    };
    let exit_ns = unix_ns_or_zero();
    let elapsed_ns = launched_at_monotonic.elapsed().as_nanos();
    let terminal_snapshot = base_event(
        "process-terminal-observed",
        exit_ns,
        supervisor_pid,
        invocation.role,
    )
    .field("child_pid", child_pid)
    .field("elapsed_ns", elapsed_ns)
    .field("snapshot_phase", "terminal-before-reap");
    let terminal_snapshot = observation.event_fields(terminal_snapshot);
    let terminal_snapshot = process::terminal_child_event_fields(terminal_snapshot, child_pid);
    append_after_spawn(
        &mut journal,
        &terminal_snapshot,
        invocation.role,
        child_pid,
        &mut retained_journal_errors,
    );

    let backtrace = match invocation.role.backtrace() {
        BacktraceKind::None => BacktraceEvidence::NotApplicable,
        BacktraceKind::LinuxCncTask => {
            backtrace::capture(journal.path(), child_pid, launched_at_wall, exit_ns)
        }
    };
    let core_artifact = core_artifact::capture(
        journal.path(),
        child_pid,
        observation.core_dumped(),
        &child_cwd,
        &fallback_cwd,
        launched_at_wall,
        exit_ns,
    );
    let evidence = match wait::reap(&mut child) {
        Ok(evidence) => evidence,
        Err(source) => {
            let event = observation.event_fields(
                base_event(
                    "reap-failed",
                    unix_ns_or_zero(),
                    supervisor_pid,
                    invocation.role,
                )
                .field("child_pid", child_pid)
                .field("error_kind", format!("{:?}", source.kind()))
                .field("raw_os_error", optional_i32(source.raw_os_error()))
                .field("error_hex", hex_bytes(source.to_string().as_bytes())),
            );
            append_after_spawn(
                &mut journal,
                &event,
                invocation.role,
                child_pid,
                &mut retained_journal_errors,
            );
            return Err(SupervisorError::Reap {
                role: invocation.role,
                child_pid,
                source,
            });
        }
    };
    let event = termination_event(
        supervisor_pid,
        invocation.role,
        exit_ns,
        elapsed_ns,
        observation,
        evidence,
        &backtrace,
        &core_artifact,
    );
    append_after_spawn(
        &mut journal,
        &event,
        invocation.role,
        child_pid,
        &mut retained_journal_errors,
    );
    if !retained_journal_errors.is_empty() {
        let first = retained_journal_errors.remove(0);
        return Err(SupervisorError::JournalAfterChildSpawn {
            first,
            additional_failures: retained_journal_errors.len(),
        });
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
        .field("core_dump_policy", role.core_dump_policy().name())
        .field("owner_comm", role.owner_comm())
        .field("argument_placement", role.argument_placement().name())
}

fn termination_event(
    supervisor_pid: u32,
    role: ProcessRole,
    exit_ns: u128,
    elapsed_ns: u128,
    observation: TerminalObservation,
    evidence: WaitEvidence,
    backtrace: &BacktraceEvidence,
    core_artifact: &CoreArtifactEvidence,
) -> Event {
    let event = base_event("process-terminated", exit_ns, supervisor_pid, role)
        .field("elapsed_ns", elapsed_ns)
        .field("waitid_wait4_consistent", observation.agrees_with(evidence));
    let event = observation.event_fields(event);
    let event = evidence.event_fields(event);
    let event = backtrace.event_fields(event);
    let event = core_artifact.event_fields(event);

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

fn append_after_spawn(
    journal: &mut Journal,
    event: &Event,
    role: ProcessRole,
    child_pid: u32,
    failures: &mut Vec<JournalError>,
) {
    if let Err(error) = journal.append(event) {
        eprintln!(
            "dmc2-process-supervisor: role={} child_pid={child_pid} lifecycle_journal_append=failed error={error} fallback_event={}",
            role.name(),
            event.render()
        );
        failures.push(error);
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

#[derive(Debug)]
pub enum SupervisorError {
    Cli(CliError),
    UnsupportedOwnership {
        role: ProcessRole,
        ownership: Ownership,
    },
    Journal(JournalError),
    JournalAfterChildSpawn {
        first: JournalError,
        additional_failures: usize,
    },
    Clock(SystemTimeError),
    CoreDumpLimit {
        role: ProcessRole,
        source: io::Error,
    },
    CoreDumpLimitAndJournal {
        role: ProcessRole,
        limit: io::Error,
        journal: JournalError,
    },
    OwnerIdentity {
        role: ProcessRole,
        source: io::Error,
    },
    OwnerIdentityAndJournal {
        role: ProcessRole,
        identity: io::Error,
        journal: JournalError,
    },
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
    Observe {
        role: ProcessRole,
        child_pid: u32,
        source: io::Error,
    },
    Reap {
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
            Self::JournalAfterChildSpawn {
                first,
                additional_failures,
            } => write!(
                formatter,
                "a supervised process ran but {} lifecycle record(s) could not be persisted; first error: {first}",
                additional_failures + 1
            ),
            Self::Clock(error) => write!(formatter, "system clock predates Unix epoch: {error}"),
            Self::CoreDumpLimit { role, source } => write!(
                formatter,
                "could not establish the core-dump capture plan for role {}: {source}",
                role.name()
            ),
            Self::CoreDumpLimitAndJournal {
                role,
                limit,
                journal,
            } => write!(
                formatter,
                "could not establish the core-dump capture plan for role {}: {limit}; the lifecycle failure record also failed: {journal}",
                role.name()
            ),
            Self::OwnerIdentity { role, source } => write!(
                formatter,
                "could not establish the durable process-owner identity for role {}: {source}",
                role.name()
            ),
            Self::OwnerIdentityAndJournal {
                role,
                identity,
                journal,
            } => write!(
                formatter,
                "could not establish the durable process-owner identity for role {}: {identity}; the lifecycle failure record also failed: {journal}",
                role.name()
            ),
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
            Self::Observe {
                role,
                child_pid,
                source,
            } => write!(
                formatter,
                "could not observe terminal state for role {} child PID {child_pid}: {source}",
                role.name()
            ),
            Self::Reap {
                role,
                child_pid,
                source,
            } => write!(
                formatter,
                "could not reap role {} child PID {child_pid} after terminal observation: {source}",
                role.name()
            ),
        }
    }
}

impl std::error::Error for SupervisorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cli(error) => Some(error),
            Self::Journal(error) => Some(error),
            Self::JournalAfterChildSpawn { first, .. } => Some(first),
            Self::Clock(error) => Some(error),
            Self::CoreDumpLimit { source, .. } => Some(source),
            Self::CoreDumpLimitAndJournal { limit, .. } => Some(limit),
            Self::OwnerIdentity { source, .. } => Some(source),
            Self::OwnerIdentityAndJournal { identity, .. } => Some(identity),
            Self::Spawn { source, .. }
            | Self::Observe { source, .. }
            | Self::Reap { source, .. } => Some(source),
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
