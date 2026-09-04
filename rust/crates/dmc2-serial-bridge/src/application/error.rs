use std::fmt;

use super::cli::CliError;
use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};
use dmc2_serial_bridge::hal::RegistrationError;

#[derive(Debug)]
pub(crate) enum ApplicationError {
    Cli(CliError),
    PacketTimeoutOverflow { milliseconds: u64 },
    SerialPathNul { position: usize },
    Hal(RegistrationError),
}

impl From<CliError> for ApplicationError {
    fn from(error: CliError) -> Self {
        Self::Cli(error)
    }
}

impl From<RegistrationError> for ApplicationError {
    fn from(error: RegistrationError) -> Self {
        Self::Hal(error)
    }
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(error) => write!(formatter, "SERIAL_BRIDGE_CLI_INVALID: {error}"),
            Self::PacketTimeoutOverflow { milliseconds } => write!(
                formatter,
                "SERIAL_BRIDGE_PACKET_TIMEOUT_OVERFLOW: milliseconds={milliseconds}; action: choose a timeout representable in nanoseconds"
            ),
            Self::SerialPathNul { position } => write!(
                formatter,
                "SERIAL_BRIDGE_DEVICE_PATH_NUL: byte={position}; action: configure a device path without an embedded NUL byte"
            ),
            Self::Hal(error) => write!(formatter, "SERIAL_BRIDGE_HAL_REGISTRATION_FAILED: {error}"),
        }
    }
}

impl RecoveryClassified for ApplicationError {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::Cli(error) => error.recovery_class(),
            Self::PacketTimeoutOverflow { .. } | Self::SerialPathNul { .. } => {
                RecoveryClass::RelaunchApplication
            }
            Self::Hal(error) => error.recovery_class(),
        }
    }
}
