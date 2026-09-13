use std::fmt;
use std::process::Command;

use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};

pub const PHYSICAL_PENDANT_ESTOP_PIN: &str = "dmc2-pendant.estop-pressed";

/// The only driver acknowledgements exposed by the Clear Fault operation.
#[derive(Clone, Copy, Debug)]
pub enum DriverReset {
    MesaIoError,
    MesaWatchdog,
}

impl DriverReset {
    fn command(self) -> [&'static str; 3] {
        match self {
            Self::MesaIoError => ["setp", "hm2_7i95.0.io_error", "FALSE"],
            // has_bit is linked to this HAL I/O signal; setp on a linked
            // pin is rejected by LinuxCNC. The signal has no HAL_OUT writer.
            Self::MesaWatchdog => ["sets", "dmc2-mesa-watchdog-fault", "FALSE"],
        }
    }
}

pub fn acknowledge_driver(reset: DriverReset) -> Result<(), HalError> {
    let args = reset.command();
    let output = Command::new("halcmd")
        .env("LINUXCNC_FORCE_REALTIME", "1")
        .args(args)
        .output()
        .map_err(|source| HalError::WriteSpawn { reset, source })?;
    if !output.status.success() {
        return Err(HalError::Write {
            reset,
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(())
}

pub fn read_bit(pin: &'static str) -> Result<bool, HalError> {
    let output = Command::new("halcmd")
        .env("LINUXCNC_FORCE_REALTIME", "1")
        .args(["-s", "getp", pin])
        .output()
        .map_err(|source| HalError::Spawn { pin, source })?;
    if !output.status.success() {
        return Err(HalError::Command {
            pin,
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let value =
        String::from_utf8(output.stdout).map_err(|source| HalError::NonUtf8 { pin, source })?;
    match value.trim() {
        "TRUE" | "1" => Ok(true),
        "FALSE" | "0" => Ok(false),
        observed => Err(HalError::InvalidBit {
            pin,
            observed: observed.to_owned(),
        }),
    }
}

#[derive(Debug)]
pub enum HalError {
    WriteSpawn {
        reset: DriverReset,
        source: std::io::Error,
    },
    Write {
        reset: DriverReset,
        status: Option<i32>,
        stderr: String,
    },
    Spawn {
        pin: &'static str,
        source: std::io::Error,
    },
    Command {
        pin: &'static str,
        status: Option<i32>,
        stderr: String,
    },
    NonUtf8 {
        pin: &'static str,
        source: std::string::FromUtf8Error,
    },
    InvalidBit {
        pin: &'static str,
        observed: String,
    },
}

impl fmt::Display for HalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WriteSpawn { reset, source } => write!(formatter,
                "cannot submit the operator's {reset:?} acknowledgement: {source}; use Clear Fault to retry"),
            Self::Write { reset, status, stderr } => write!(formatter,
                "driver acknowledgement {reset:?} was rejected: status={status:?} detail={stderr}; correct the reported HAL cause and retry Clear Fault"),
            Self::Spawn { pin, source } => {
                write!(formatter, "cannot read HAL pin {pin}: {source}")
            }
            Self::Command {
                pin,
                status,
                stderr,
            } => write!(
                formatter,
                "HAL read failed for {pin}: status={status:?} detail={stderr:?}"
            ),
            Self::NonUtf8 { pin, source } => {
                write!(formatter, "HAL pin {pin} returned non-UTF-8: {source}")
            }
            Self::InvalidBit { pin, observed } => {
                write!(formatter, "HAL pin {pin} returned invalid bit {observed:?}")
            }
        }
    }
}

impl RecoveryClassified for HalError {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::WriteSpawn { .. } | Self::Write { .. } => RecoveryClass::ClearController,
            Self::Spawn { .. }
            | Self::Command { .. }
            | Self::NonUtf8 { .. }
            | Self::InvalidBit { .. } => RecoveryClass::RelaunchApplication,
        }
    }
}
