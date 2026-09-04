use std::fmt;

use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};

use super::cli::CliError;
use super::runtime::NativeRuntimeError;

#[derive(Debug)]
pub(super) enum ApplicationError {
    Cli(CliError),
    TaskStatusAbiMismatch {
        native_version: u32,
        native_size: usize,
        rust_version: u32,
        rust_size: usize,
    },
    ErrorMessageAbiMismatch {
        native_version: u32,
        native_size: usize,
        rust_version: u32,
        rust_size: usize,
    },
    Runtime(NativeRuntimeError),
}

impl ApplicationError {
    pub(super) const fn identity(&self) -> &'static str {
        match self {
            Self::Cli(_) => "TASK_MONITOR_CLI_INVALID",
            Self::TaskStatusAbiMismatch { .. } => "TASK_STATUS_ABI_MISMATCH",
            Self::ErrorMessageAbiMismatch { .. } => "ERROR_MESSAGE_ABI_MISMATCH",
            Self::Runtime(error) => error.identity(),
        }
    }
}

impl From<CliError> for ApplicationError {
    fn from(error: CliError) -> Self {
        Self::Cli(error)
    }
}

impl From<NativeRuntimeError> for ApplicationError {
    fn from(error: NativeRuntimeError) -> Self {
        Self::Runtime(error)
    }
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(error) => write!(formatter, "{}: {error}", self.identity()),
            Self::TaskStatusAbiMismatch {
                native_version,
                native_size,
                rust_version,
                rust_size,
            } => write!(
                formatter,
                "{}: C++ version=0x{native_version:08x} size={native_size}, Rust version=0x{rust_version:08x} size={rust_size}; cause: the native and Rust status layouts differ; action: stop the monitor and rebuild/reinstall it against pinned LinuxCNC 2.9.10",
                self.identity()
            ),
            Self::ErrorMessageAbiMismatch {
                native_version,
                native_size,
                rust_version,
                rust_size,
            } => write!(
                formatter,
                "{}: C++ version=0x{native_version:08x} size={native_size}, Rust version=0x{rust_version:08x} size={rust_size}; cause: the native and Rust LinuxCNC error-object layouts differ; action: stop the monitor and rebuild/reinstall it against pinned LinuxCNC 2.9.10",
                self.identity()
            ),
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}

impl RecoveryClassified for ApplicationError {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::Cli(error) => error.recovery_class(),
            Self::TaskStatusAbiMismatch { .. } | Self::ErrorMessageAbiMismatch { .. } => {
                RecoveryClass::RelaunchApplication
            }
            Self::Runtime(error) => error.recovery_class(),
        }
    }
}
