use std::fmt;

use crate::application::journal_error::JournalError;

use super::RegistrationError;

#[derive(Debug)]
pub(in crate::application) enum PublisherError {
    Registration(RegistrationError),
    DiagnosticJournal(JournalError),
}

impl From<RegistrationError> for PublisherError {
    fn from(error: RegistrationError) -> Self {
        Self::Registration(error)
    }
}

impl From<JournalError> for PublisherError {
    fn from(error: JournalError) -> Self {
        Self::DiagnosticJournal(error)
    }
}

impl fmt::Display for PublisherError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registration(error) => {
                write!(formatter, "TASK_MONITOR_HAL_REGISTRATION_FAILED: {error}")
            }
            Self::DiagnosticJournal(error) => {
                write!(formatter, "TASK_MONITOR_DIAGNOSTIC_JOURNAL_FAILED: {error}")
            }
        }
    }
}
