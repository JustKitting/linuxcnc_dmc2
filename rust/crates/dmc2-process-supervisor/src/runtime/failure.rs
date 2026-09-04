use std::ffi::OsString;
use std::fmt;
use std::io;
use std::time::SystemTimeError;

use crate::catalog::{Ownership, ProcessRole};
use crate::cli::CliError;
use crate::journal::JournalError;

#[derive(Debug, Clone, Copy)]
pub enum StartupStage {
    OwnerIdentity,
    CoreDumpLimit,
    Spawn,
}

impl StartupStage {
    fn name(self) -> &'static str {
        match self {
            Self::OwnerIdentity => "set the process-owner identity",
            Self::CoreDumpLimit => "configure the child core-dump limit",
            Self::Spawn => "start the supervised process",
        }
    }
}

#[derive(Debug)]
pub enum SupervisorError {
    Cli(CliError),
    UnsupportedOwnership {
        role: ProcessRole,
        ownership: Ownership,
    },
    Journal(JournalError),
    Clock(SystemTimeError),
    Startup {
        stage: StartupStage,
        role: ProcessRole,
        program: OsString,
        source: io::Error,
        journal: Option<JournalError>,
    },
    WaitStatusLost {
        role: ProcessRole,
        child_pid: u32,
        source: io::Error,
        journal_failures: u64,
    },
    JournalAfterChildSpawn {
        first: JournalError,
        additional_failures: u64,
    },
}

impl SupervisorError {
    pub fn startup(
        stage: StartupStage,
        role: ProcessRole,
        program: OsString,
        source: io::Error,
        journal: Option<JournalError>,
    ) -> Self {
        Self::Startup {
            stage,
            role,
            program,
            source,
            journal,
        }
    }
}

impl fmt::Display for SupervisorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(error) => write!(formatter, "invalid supervisor invocation: {error}; recovery: relaunch LinuxCNC from the desktop application"),
            Self::UnsupportedOwnership { role, ownership } => write!(formatter, "role {} has ownership {}, not direct-child; recovery: restore the reviewed process catalog and relaunch from the desktop application", role.name(), ownership.name()),
            Self::Journal(error) => write!(formatter, "lifecycle journal unavailable before process start: {error}"),
            Self::Clock(error) => write!(formatter, "system clock predates the Unix epoch: {error}; recovery: correct system time and relaunch from the desktop application"),
            Self::Startup { stage, role, program, source, journal } => {
                write!(formatter, "could not {} for role {} program {program:?}: {source}", stage.name(), role.name())?;
                if let Some(journal) = journal { write!(formatter, "; the lifecycle failure record also failed: {journal}")?; }
                write!(formatter, "; recovery: correct the named startup error and relaunch LinuxCNC from the desktop application")
            }
            Self::WaitStatusLost { role, child_pid, source, journal_failures } => write!(formatter, "lost the terminal status of role {} child PID {child_pid} after wait4 failed: {source}; lifecycle journal failures={journal_failures}; recovery: use the visible Clear Fault, Home, and Pendant Mode controls if AXIS remains open, otherwise relaunch from the desktop application", role.name()),
            Self::JournalAfterChildSpawn { first, additional_failures } => write!(formatter, "the supervised process remained owned through termination, but {} lifecycle record(s) failed; first error: {first}; recovery: correct log storage and relaunch from the desktop application", additional_failures + 1),
        }
    }
}

impl std::error::Error for SupervisorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cli(error) => Some(error),
            Self::Journal(error) => Some(error),
            Self::Clock(error) => Some(error),
            Self::Startup { source, .. } | Self::WaitStatusLost { source, .. } => Some(source),
            Self::JournalAfterChildSpawn { first, .. } => Some(first),
            Self::UnsupportedOwnership { .. } => None,
        }
    }
}
