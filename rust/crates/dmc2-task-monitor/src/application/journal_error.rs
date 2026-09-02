//! Typed filesystem failures shared by both durable task-monitor journals.

use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::application) enum JournalKind {
    LinuxCncErrorChannel,
    Diagnostic,
}

impl JournalKind {
    const fn name(self) -> &'static str {
        match self {
            Self::LinuxCncErrorChannel => "linuxcnc-error-channel",
            Self::Diagnostic => "diagnostic",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::application) enum AtomicPublishStep {
    WriteHeader,
    SyncTemporary,
    Rename,
    OpenParent,
    SyncParent,
}

#[derive(Debug)]
pub(in crate::application) enum JournalError {
    ParentUnavailable {
        kind: JournalKind,
        path: PathBuf,
        source: io::Error,
    },
    ParentNotDirectory {
        kind: JournalKind,
        path: PathBuf,
    },
    TargetNotRegular {
        kind: JournalKind,
        path: PathBuf,
    },
    TargetInspectionFailed {
        kind: JournalKind,
        path: PathBuf,
        source: io::Error,
    },
    FilenameMissing {
        kind: JournalKind,
        path: PathBuf,
    },
    TemporaryCreateFailed {
        kind: JournalKind,
        path: PathBuf,
        source: io::Error,
    },
    TemporaryNamesExhausted {
        kind: JournalKind,
        path: PathBuf,
    },
    AtomicPublishFailed {
        kind: JournalKind,
        path: PathBuf,
        step: AtomicPublishStep,
        source: io::Error,
        cleanup: Option<(PathBuf, io::Error)>,
    },
    SequenceExhausted {
        kind: JournalKind,
        path: PathBuf,
    },
    AppendFailed {
        kind: JournalKind,
        path: PathBuf,
        source: io::Error,
    },
    SyncFailed {
        kind: JournalKind,
        path: PathBuf,
        source: io::Error,
    },
}

impl JournalError {
    pub(in crate::application) const fn identity(&self) -> &'static str {
        match self {
            Self::ParentUnavailable { .. } => "JOURNAL_PARENT_UNAVAILABLE",
            Self::ParentNotDirectory { .. } => "JOURNAL_PARENT_NOT_DIRECTORY",
            Self::TargetNotRegular { .. } => "JOURNAL_TARGET_NOT_REGULAR",
            Self::TargetInspectionFailed { .. } => "JOURNAL_TARGET_INSPECTION_FAILED",
            Self::FilenameMissing { .. } => "JOURNAL_FILENAME_MISSING",
            Self::TemporaryCreateFailed { .. } => "JOURNAL_TEMPORARY_CREATE_FAILED",
            Self::TemporaryNamesExhausted { .. } => "JOURNAL_TEMPORARY_NAMES_EXHAUSTED",
            Self::AtomicPublishFailed { .. } => "JOURNAL_ATOMIC_PUBLISH_FAILED",
            Self::SequenceExhausted { .. } => "JOURNAL_SEQUENCE_EXHAUSTED",
            Self::AppendFailed { .. } => "JOURNAL_APPEND_FAILED",
            Self::SyncFailed { .. } => "JOURNAL_SYNC_FAILED",
        }
    }

    pub(in crate::application) const fn kind(&self) -> JournalKind {
        match self {
            Self::ParentUnavailable { kind, .. }
            | Self::ParentNotDirectory { kind, .. }
            | Self::TargetNotRegular { kind, .. }
            | Self::TargetInspectionFailed { kind, .. }
            | Self::FilenameMissing { kind, .. }
            | Self::TemporaryCreateFailed { kind, .. }
            | Self::TemporaryNamesExhausted { kind, .. }
            | Self::AtomicPublishFailed { kind, .. }
            | Self::SequenceExhausted { kind, .. }
            | Self::AppendFailed { kind, .. }
            | Self::SyncFailed { kind, .. } => *kind,
        }
    }

    const fn action(&self) -> &'static str {
        match self {
            Self::ParentUnavailable { .. }
            | Self::ParentNotDirectory { .. }
            | Self::TargetNotRegular { .. }
            | Self::TargetInspectionFailed { .. }
            | Self::FilenameMissing { .. }
            | Self::TemporaryCreateFailed { .. }
            | Self::TemporaryNamesExhausted { .. }
            | Self::AtomicPublishFailed { .. } => {
                "restore the configured journal path and filesystem integrity before restarting the monitor"
            }
            Self::SequenceExhausted { .. } => {
                "retain the completed journal and restart the monitor with a new journal"
            }
            Self::AppendFailed { .. } | Self::SyncFailed { .. } => {
                "restore durable storage before restarting the monitor"
            }
        }
    }
}

impl fmt::Display for JournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: kind={}",
            self.identity(),
            self.kind().name()
        )?;
        match self {
            Self::ParentUnavailable { path, source, .. }
            | Self::TargetInspectionFailed { path, source, .. }
            | Self::TemporaryCreateFailed { path, source, .. }
            | Self::AppendFailed { path, source, .. }
            | Self::SyncFailed { path, source, .. } => {
                write!(formatter, " path={} source={source}", path.display())?;
            }
            Self::ParentNotDirectory { path, .. }
            | Self::TargetNotRegular { path, .. }
            | Self::FilenameMissing { path, .. }
            | Self::TemporaryNamesExhausted { path, .. }
            | Self::SequenceExhausted { path, .. } => {
                write!(formatter, " path={}", path.display())?;
            }
            Self::AtomicPublishFailed {
                path,
                step,
                source,
                cleanup,
                ..
            } => {
                write!(
                    formatter,
                    " path={} step={step:?} source={source}",
                    path.display()
                )?;
                if let Some((cleanup_path, cleanup_error)) = cleanup {
                    write!(
                        formatter,
                        " cleanup_path={} cleanup_source={cleanup_error}",
                        cleanup_path.display()
                    )?;
                }
            }
        }
        write!(formatter, "; action: {}", self.action())
    }
}
