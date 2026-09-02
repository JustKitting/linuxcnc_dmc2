//! Typed failures shared by userspace HAL component registration paths.

use core::fmt;

use crate::HalError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HalNameKind {
    Component,
    Pin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HalRegistrationFailure<Context> {
    NameTooLong {
        kind: HalNameKind,
        context: Context,
        length: usize,
        maximum: usize,
    },
    InvalidCString {
        kind: HalNameKind,
        context: Context,
        nul_position: usize,
    },
    Call {
        error: HalError,
        context: Option<Context>,
    },
    Allocation {
        bytes: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HalRegistrationError<Context> {
    failure: HalRegistrationFailure<Context>,
    cleanup: Option<HalError>,
}

impl<Context> HalRegistrationError<Context> {
    pub const fn new(failure: HalRegistrationFailure<Context>) -> Self {
        Self {
            failure,
            cleanup: None,
        }
    }

    pub const fn failure(&self) -> &HalRegistrationFailure<Context> {
        &self.failure
    }

    pub const fn cleanup(&self) -> Option<HalError> {
        self.cleanup
    }

    pub fn with_cleanup(mut self, cleanup: HalError) -> Self {
        self.cleanup = Some(cleanup);
        self
    }
}

impl<Context: fmt::Display> fmt::Display for HalRegistrationError<Context> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.failure {
            HalRegistrationFailure::NameTooLong {
                kind: HalNameKind::Component,
                length,
                maximum,
                ..
            } => write!(
                formatter,
                "HAL component name is {length} bytes; LinuxCNC permits at most {maximum}"
            )?,
            HalRegistrationFailure::NameTooLong {
                kind: HalNameKind::Pin,
                context,
                length,
                maximum,
            } => write!(
                formatter,
                "HAL pin name for {context} is {length} bytes; LinuxCNC permits at most {maximum}"
            )?,
            HalRegistrationFailure::InvalidCString {
                kind: HalNameKind::Component,
                ..
            } => formatter.write_str("HAL component name contained a NUL byte")?,
            HalRegistrationFailure::InvalidCString {
                kind: HalNameKind::Pin,
                ..
            } => formatter.write_str("HAL pin name contained a NUL byte")?,
            HalRegistrationFailure::Call {
                error,
                context: Some(context),
            } => write!(
                formatter,
                "{}({context}) failed: {error}",
                error.call().name()
            )?,
            HalRegistrationFailure::Call {
                error,
                context: None,
            } => write!(formatter, "{} failed: {error}", error.call().name())?,
            HalRegistrationFailure::Allocation { .. } => {
                formatter.write_str("hal_malloc for pin-pointer storage failed")?
            }
        }
        if let Some(cleanup) = self.cleanup {
            write!(formatter, "; hal_exit cleanup failed: {cleanup}")?;
        }
        Ok(())
    }
}
