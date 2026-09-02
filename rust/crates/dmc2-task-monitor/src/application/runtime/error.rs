use std::ffi::NulError;
use std::fmt;

use crate::application::hal::PublisherError;
use crate::application::journal_error::JournalError;

#[derive(Debug)]
pub(in crate::application) enum RuntimeError {
    ErrorJournal(JournalError),
    HalPublication(PublisherError),
    FailClosedPublication {
        primary: JournalError,
        publication: PublisherError,
    },
}

impl RuntimeError {
    pub(super) const fn identity(&self) -> &'static str {
        match self {
            Self::ErrorJournal(_) => "RUNTIME_ERROR_JOURNAL_FAILED",
            Self::HalPublication(_) => "RUNTIME_HAL_PUBLICATION_FAILED",
            Self::FailClosedPublication { .. } => "RUNTIME_FAIL_CLOSED_PUBLICATION_FAILED",
        }
    }
}

impl From<PublisherError> for RuntimeError {
    fn from(error: PublisherError) -> Self {
        Self::HalPublication(error)
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ErrorJournal(error) => {
                write!(formatter, "{}: {error}", self.identity())
            }
            Self::HalPublication(error) => {
                write!(formatter, "{}: {error}", self.identity())
            }
            Self::FailClosedPublication {
                primary,
                publication,
            } => write!(
                formatter,
                "{}: primary={primary}; publication={publication}; action: restore both journals and restart the monitor",
                self.identity()
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::application) enum RequiredRuntimePath {
    ErrorJournal,
    DiagnosticJournal,
}

impl RequiredRuntimePath {
    const fn option(self) -> &'static str {
        match self {
            Self::ErrorJournal => "--error-journal",
            Self::DiagnosticJournal => "--diagnostic-journal",
        }
    }

    const fn purpose(self) -> &'static str {
        match self {
            Self::ErrorJournal => "lossless error ownership",
            Self::DiagnosticJournal => "self-describing diagnostics",
        }
    }
}

#[derive(Debug)]
pub(in crate::application) enum NativeRuntimeError {
    MissingRequiredPath(RequiredRuntimePath),
    ErrorJournal(JournalError),
    NmlPathContainsNul(NulError),
    Hal(PublisherError),
    Runtime(RuntimeError),
}

impl NativeRuntimeError {
    pub(in crate::application) const fn identity(&self) -> &'static str {
        match self {
            Self::MissingRequiredPath(_) => "RUNTIME_REQUIRED_PATH_MISSING",
            Self::ErrorJournal(_) => "RUNTIME_ERROR_JOURNAL_STARTUP_FAILED",
            Self::NmlPathContainsNul(_) => "RUNTIME_NML_PATH_CONTAINS_NUL",
            Self::Hal(_) => "RUNTIME_HAL_STARTUP_FAILED",
            Self::Runtime(error) => error.identity(),
        }
    }
}

impl From<JournalError> for NativeRuntimeError {
    fn from(error: JournalError) -> Self {
        Self::ErrorJournal(error)
    }
}

impl From<PublisherError> for NativeRuntimeError {
    fn from(error: PublisherError) -> Self {
        Self::Hal(error)
    }
}

impl From<RuntimeError> for NativeRuntimeError {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(error)
    }
}

impl fmt::Display for NativeRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequiredPath(path) => write!(
                formatter,
                "{}: runtime task monitor requires {} PATH for {}",
                self.identity(),
                path.option(),
                path.purpose()
            ),
            Self::ErrorJournal(error) | Self::Hal(PublisherError::DiagnosticJournal(error)) => {
                write!(formatter, "{}: {error}", self.identity())
            }
            Self::NmlPathContainsNul(error) => write!(
                formatter,
                "{}: byte_offset={}",
                self.identity(),
                error.nul_position()
            ),
            Self::Hal(error) => write!(formatter, "{}: {error}", self.identity()),
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}
