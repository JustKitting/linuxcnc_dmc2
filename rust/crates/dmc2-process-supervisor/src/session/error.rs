use std::ffi::OsString;
use std::fmt;
use std::io;
use std::time::SystemTimeError;

use crate::catalog::{Ownership, ProcessRole};
use crate::cli::CliError;
use crate::journal::JournalError;

use super::optional_i32;

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
        additional_failures: u64,
    },
    EnableSubreaper(io::Error),
    VerifySubreaper(io::Error),
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
    OutputCapture {
        operation: &'static str,
        source: io::Error,
    },
    LinuxCncStatusMissing {
        linuxcnc_pid: u32,
    },
    TerminalObservationDegraded {
        waitid_error_kind: io::ErrorKind,
        waitid_raw_os_error: Option<i32>,
        waitid_error: String,
        fallback_wait4_failures: u64,
        retained_reap_degraded_children: usize,
        retained_reap_wait4_failures: u64,
        first_journal_failure: Option<JournalError>,
        additional_journal_failures: u64,
    },
    TerminalReapDegraded {
        child_pid: u32,
        additional_affected_children: usize,
        first_error_kind: io::ErrorKind,
        first_raw_os_error: Option<i32>,
        first_error: String,
        wait4_failures: u64,
        first_journal_failure: Option<JournalError>,
        additional_journal_failures: u64,
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
                "could not establish the durable session-owner identity for role {}: {source}",
                role.name()
            ),
            Self::OwnerIdentityAndJournal {
                role,
                identity,
                journal,
            } => write!(
                formatter,
                "could not establish the durable session-owner identity for role {}: {identity}; the lifecycle failure record also failed: {journal}",
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
            Self::OutputCapture { operation, source } => write!(
                formatter,
                "LinuxCNC session output capture could not {operation}: {source}"
            ),
            Self::LinuxCncStatusMissing { linuxcnc_pid } => write!(
                formatter,
                "no children remain but LinuxCNC PID {linuxcnc_pid} had no captured wait status"
            ),
            Self::TerminalObservationDegraded {
                waitid_error_kind,
                waitid_raw_os_error,
                waitid_error,
                fallback_wait4_failures,
                retained_reap_degraded_children,
                retained_reap_wait4_failures,
                first_journal_failure,
                additional_journal_failures,
            } => write!(
                formatter,
                "session waitid observation failed: kind={waitid_error_kind:?}, raw_os_error={}, detail={waitid_error}; the subreaper retained and reaped all children through wait4 before returning tracking failure; fallback wait4 polling failures={fallback_wait4_failures}; retained-terminal reap-degraded children={retained_reap_degraded_children}; retained-terminal wait4 failures={retained_reap_wait4_failures}; journal failures={}{}",
                optional_i32(*waitid_raw_os_error),
                u64::from(first_journal_failure.is_some()) + *additional_journal_failures,
                first_journal_failure
                    .as_ref()
                    .map_or_else(String::new, |error| format!(" (first: {error})")),
            ),
            Self::TerminalReapDegraded {
                child_pid,
                additional_affected_children,
                first_error_kind,
                first_raw_os_error,
                first_error,
                wait4_failures,
                first_journal_failure,
                additional_journal_failures,
            } => write!(
                formatter,
                "wait4 initially failed after waitid retained session child PID {child_pid}: kind={first_error_kind:?}, raw_os_error={}, detail={first_error}; the subreaper kept the exact child waitable and eventually reaped its real status; wait4 failures={wait4_failures}; additional affected children={additional_affected_children}; journal failures={}{}",
                optional_i32(*first_raw_os_error),
                u64::from(first_journal_failure.is_some()) + *additional_journal_failures,
                first_journal_failure
                    .as_ref()
                    .map_or_else(String::new, |error| format!(" (first: {error})")),
            ),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        first_source(self)
    }
}

fn first_source(error: &SessionError) -> Option<&(dyn std::error::Error + 'static)> {
    match error {
        SessionError::Cli(error) => Some(error),
        SessionError::Journal(error) => Some(error),
        SessionError::JournalAfterSpawn { first, .. } => Some(first),
        SessionError::EnableSubreaper(error)
        | SessionError::VerifySubreaper(error)
        | SessionError::CoreDumpLimit { source: error, .. }
        | SessionError::OwnerIdentity { source: error, .. }
        | SessionError::OutputCapture { source: error, .. }
        | SessionError::Spawn { source: error, .. }
        | SessionError::SpawnAndJournal { spawn: error, .. } => Some(error),
        SessionError::UnsupportedOwnership { .. }
        | SessionError::LinuxCncStatusMissing { .. }
        | SessionError::TerminalObservationDegraded { .. }
        | SessionError::TerminalReapDegraded { .. } => None,
        SessionError::CoreDumpLimitAndJournal { limit, .. } => Some(limit),
        SessionError::OwnerIdentityAndJournal { identity, .. } => Some(identity),
        SessionError::Clock(error) => Some(error),
    }
}
