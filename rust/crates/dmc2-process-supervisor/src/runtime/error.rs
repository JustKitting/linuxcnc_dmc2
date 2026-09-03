use std::ffi::OsString;
use std::fmt;
use std::io;
use std::time::SystemTimeError;

use crate::catalog::{Ownership, ProcessRole};
use crate::cli::CliError;
use crate::journal::JournalError;
use crate::signal_evidence;

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
        additional_failures: u64,
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
    SignalEvidenceSetup {
        role: ProcessRole,
        source: signal_evidence::Error,
    },
    SignalEvidenceSetupAndJournal {
        role: ProcessRole,
        setup: signal_evidence::Error,
        journal: JournalError,
    },
    SignalEvidenceAfterChildSpawn {
        role: ProcessRole,
        child_pid: u32,
        failures: signal_evidence::InfrastructureFailures,
    },
    JournalAndSignalEvidenceAfterChildSpawn {
        first: JournalError,
        additional_journal_failures: u64,
        signal: signal_evidence::InfrastructureFailures,
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
    TerminalObservationDegraded {
        role: ProcessRole,
        child_pid: u32,
        waitid_error_kind: io::ErrorKind,
        waitid_raw_os_error: Option<i32>,
        waitid_error: String,
        fallback_wait4_failures: u64,
        first_journal_failure: Option<JournalError>,
        additional_journal_failures: u64,
        signal_failures: Option<signal_evidence::InfrastructureFailures>,
    },
    TerminalReapDegraded {
        role: ProcessRole,
        child_pid: u32,
        first_error_kind: io::ErrorKind,
        first_raw_os_error: Option<i32>,
        first_error: String,
        wait4_failures: u64,
        first_journal_failure: Option<JournalError>,
        additional_journal_failures: u64,
        signal_failures: Option<signal_evidence::InfrastructureFailures>,
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
            Self::SignalEvidenceSetup { role, source } => write!(
                formatter,
                "could not establish caught-signal evidence for role {}: {source}",
                role.name()
            ),
            Self::SignalEvidenceSetupAndJournal {
                role,
                setup,
                journal,
            } => write!(
                formatter,
                "could not establish caught-signal evidence for role {}: {setup}; the setup-failure lifecycle record also failed: {journal}",
                role.name()
            ),
            Self::SignalEvidenceAfterChildSpawn {
                role,
                child_pid,
                failures,
            } => write!(
                formatter,
                "caught-signal evidence failed while supervising role {} child PID {child_pid}: {failures}",
                role.name()
            ),
            Self::JournalAndSignalEvidenceAfterChildSpawn {
                first,
                additional_journal_failures,
                signal,
            } => write!(
                formatter,
                "a supervised process ran but {} lifecycle record(s) could not be persisted and caught-signal evidence failed ({signal}); first journal error: {first}",
                additional_journal_failures + 1
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
            Self::TerminalObservationDegraded {
                role,
                child_pid,
                waitid_error_kind,
                waitid_raw_os_error,
                waitid_error,
                fallback_wait4_failures,
                first_journal_failure,
                additional_journal_failures,
                signal_failures,
            } => write!(
                formatter,
                "waitid terminal observation failed for role {} child PID {child_pid}: kind={waitid_error_kind:?}, raw_os_error={}, detail={waitid_error}; the owner retained the process through the reaping wait4 fallback, but the pre-reap terminal snapshot is unavailable; fallback wait4 polling failures={fallback_wait4_failures}; journal failures={}{}; caught-signal evidence failures={}",
                role.name(),
                optional_i32(*waitid_raw_os_error),
                u64::from(first_journal_failure.is_some()) + *additional_journal_failures,
                first_journal_failure
                    .as_ref()
                    .map_or_else(String::new, |error| format!(" (first: {error})")),
                signal_failures.as_ref().map_or_else(
                    || "NONE".to_owned(),
                    ToString::to_string,
                ),
            ),
            Self::TerminalReapDegraded {
                role,
                child_pid,
                first_error_kind,
                first_raw_os_error,
                first_error,
                wait4_failures,
                first_journal_failure,
                additional_journal_failures,
                signal_failures,
            } => write!(
                formatter,
                "wait4 initially failed after waitid retained role {} child PID {child_pid}: kind={first_error_kind:?}, raw_os_error={}, detail={first_error}; the owner kept the exact child waitable and eventually reaped its real status; wait4 failures={wait4_failures}; journal failures={}{}; caught-signal evidence failures={}",
                role.name(),
                optional_i32(*first_raw_os_error),
                u64::from(first_journal_failure.is_some()) + *additional_journal_failures,
                first_journal_failure
                    .as_ref()
                    .map_or_else(String::new, |error| format!(" (first: {error})")),
                signal_failures.as_ref().map_or_else(
                    || "NONE".to_owned(),
                    ToString::to_string,
                ),
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
            Self::SignalEvidenceSetup { source, .. } => Some(source),
            Self::SignalEvidenceSetupAndJournal { setup, .. } => Some(setup),
            Self::JournalAndSignalEvidenceAfterChildSpawn { first, .. } => Some(first),
            Self::OwnerIdentity { source, .. } => Some(source),
            Self::OwnerIdentityAndJournal { identity, .. } => Some(identity),
            Self::Spawn { source, .. } => Some(source),
            Self::SpawnAndJournal { spawn, .. } => Some(spawn),
            Self::UnsupportedOwnership { .. }
            | Self::SignalEvidenceAfterChildSpawn { .. }
            | Self::TerminalObservationDegraded { .. }
            | Self::TerminalReapDegraded { .. } => None,
        }
    }
}

fn optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "NONE".to_owned(), |value| value.to_string())
}
