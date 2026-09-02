use std::fmt;
use std::process::Command;

pub const PHYSICAL_PENDANT_ESTOP_PIN: &str = "dmc2-pendant.estop-pressed";

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
