use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::SystemTimeError;

use crate::catalog::{Ownership, ProcessRole};
use crate::cli::CliError;
use crate::journal::JournalError;

#[derive(Debug, Clone, Copy)]
pub enum StartupStage {
    OwnerIdentity,
    CoreDumpLimit,
    EnableSubreaper,
    VerifySubreaper,
    PrepareOutputCapture,
    Spawn,
}

impl StartupStage {
    fn name(self) -> &'static str {
        match self {
            Self::OwnerIdentity => "set the session-owner identity",
            Self::CoreDumpLimit => "configure the LinuxCNC core-dump limit",
            Self::EnableSubreaper => "enable Linux child-subreaper ownership",
            Self::VerifySubreaper => "verify Linux child-subreaper ownership",
            Self::PrepareOutputCapture => "prepare LinuxCNC output capture",
            Self::Spawn => "start LinuxCNC",
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
    Clock(SystemTimeError),
    Startup {
        stage: StartupStage,
        role: ProcessRole,
        program: OsString,
        source: io::Error,
        journal: Option<JournalError>,
    },
    LinuxCncStatusMissing {
        linuxcnc_pid: u32,
    },
    OutputCaptureAfterSpawn {
        report_path: Option<PathBuf>,
        source: io::Error,
        additional_failures: u64,
        journal_failures: u64,
    },
    JournalAfterSpawn {
        first: JournalError,
        additional_failures: u64,
    },
}

impl SessionError {
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

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(error) => write!(formatter, "invalid session-supervisor invocation: {error}; recovery: relaunch LinuxCNC from the desktop application"),
            Self::UnsupportedOwnership { role, ownership } => write!(formatter, "role {} has ownership {}, not session-root; recovery: restore the reviewed process catalog and relaunch from the desktop application", role.name(), ownership.name()),
            Self::Journal(error) => write!(formatter, "lifecycle journal unavailable before LinuxCNC start: {error}"),
            Self::Clock(error) => write!(formatter, "system clock predates the Unix epoch: {error}; recovery: correct system time and relaunch from the desktop application"),
            Self::Startup { stage, role, program, source, journal } => {
                write!(formatter, "could not {} for role {} program {program:?}: {source}", stage.name(), role.name())?;
                if let Some(journal) = journal { write!(formatter, "; the lifecycle failure record also failed: {journal}")?; }
                write!(formatter, "; recovery: correct the named startup error and relaunch LinuxCNC from the desktop application")
            }
            Self::LinuxCncStatusMissing { linuxcnc_pid } => write!(formatter, "all session children are gone, but LinuxCNC PID {linuxcnc_pid} has no retained terminal status; recovery: relaunch from the desktop application, then use the visible Clear Fault, Home, and Pendant Mode controls"),
            Self::OutputCaptureAfterSpawn { report_path, source, additional_failures, journal_failures } => write!(formatter, "LinuxCNC remained owned through session termination, but output/report capture failed {} time(s): {source}; report path={}; lifecycle journal failures={journal_failures}; recovery: correct the named log/report storage error and relaunch from the desktop application", additional_failures + 1, report_path.as_ref().map_or_else(|| "NONE".to_owned(), |path| path.display().to_string())),
            Self::JournalAfterSpawn { first, additional_failures } => write!(formatter, "LinuxCNC remained owned through session termination, but {} lifecycle record(s) failed; first error: {first}; recovery: correct log storage and relaunch from the desktop application", additional_failures + 1),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cli(error) => Some(error),
            Self::Journal(error) => Some(error),
            Self::Clock(error) => Some(error),
            Self::Startup { source, .. } | Self::OutputCaptureAfterSpawn { source, .. } => {
                Some(source)
            }
            Self::JournalAfterSpawn { first, .. } => Some(first),
            Self::UnsupportedOwnership { .. } | Self::LinuxCncStatusMissing { .. } => None,
        }
    }
}
