use std::env;
use std::ffi::OsString;
use std::fmt;

use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};

const DEFAULT_COMPONENT: &str = "dmc2-pendant";
const DEFAULT_PORT: &str = "/dev/ttyUSB0";
const DEFAULT_BAUD: u32 = 115_200;
pub(super) const DEFAULT_TIMEOUT_MS: u64 = 100;

#[derive(Debug)]
pub(super) struct Arguments {
    pub(super) component: String,
    pub(super) port: String,
    pub(super) baud: u32,
    pub(super) timeout_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CliError {
    NonUtf8Argument { index: usize, value: OsString },
    MissingValue { option: String },
    InvalidUnsigned { option: &'static str, value: String },
    UnknownArgument { argument: String },
    ZeroPacketTimeout,
    UnsupportedBaud { observed: u32 },
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonUtf8Argument { index, value } => write!(
                formatter,
                "SERIAL_BRIDGE_ARGUMENT_NOT_UTF8: index={} value={value:?}; action: pass UTF-8 command-line arguments",
                index + 1
            ),
            Self::MissingValue { option } => write!(formatter, "{option} requires a value"),
            Self::InvalidUnsigned { option, .. } => {
                write!(formatter, "{option} must be an unsigned integer")
            }
            Self::UnknownArgument { argument } => write!(formatter, "unknown argument: {argument}"),
            Self::ZeroPacketTimeout => {
                formatter.write_str("--packet-timeout-ms must be positive")
            }
            Self::UnsupportedBaud { .. } => {
                formatter.write_str("this audited bridge accepts exactly 115200 baud")
            }
        }
    }
}

impl RecoveryClassified for CliError {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::NonUtf8Argument { .. }
            | Self::MissingValue { .. }
            | Self::InvalidUnsigned { .. }
            | Self::UnknownArgument { .. }
            | Self::ZeroPacketTimeout
            | Self::UnsupportedBaud { .. } => RecoveryClass::RelaunchApplication,
        }
    }
}

pub(super) fn arguments() -> Result<Arguments, CliError> {
    parse(env::args_os().skip(1))
}

fn next_value(
    items: &mut impl Iterator<Item = (usize, OsString)>,
    option: &str,
) -> Result<String, CliError> {
    let Some((index, raw_value)) = items.next() else {
        return Err(CliError::MissingValue {
            option: option.to_owned(),
        });
    };
    raw_value
        .into_string()
        .map_err(|value| CliError::NonUtf8Argument { index, value })
}

fn parse(items: impl IntoIterator<Item = OsString>) -> Result<Arguments, CliError> {
    let mut result = Arguments {
        component: DEFAULT_COMPONENT.to_owned(),
        port: DEFAULT_PORT.to_owned(),
        baud: DEFAULT_BAUD,
        timeout_ms: DEFAULT_TIMEOUT_MS,
    };
    let mut items = items.into_iter().enumerate();
    while let Some((index, raw_argument)) = items.next() {
        let argument = raw_argument
            .into_string()
            .map_err(|value| CliError::NonUtf8Argument { index, value })?;
        match argument.as_str() {
            "--component" => result.component = next_value(&mut items, &argument)?,
            "--port" => result.port = next_value(&mut items, &argument)?,
            "--baud" => {
                let raw = next_value(&mut items, &argument)?;
                result.baud = raw.parse().map_err(|_| CliError::InvalidUnsigned {
                    option: "--baud",
                    value: raw,
                })?;
            }
            "--packet-timeout-ms" => {
                let raw = next_value(&mut items, &argument)?;
                result.timeout_ms = raw.parse().map_err(|_| CliError::InvalidUnsigned {
                    option: "--packet-timeout-ms",
                    value: raw,
                })?;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: dmc2-serial-bridge [--component NAME] [--port PATH] \
                     [--baud 115200] [--packet-timeout-ms N]"
                );
                std::process::exit(0);
            }
            _ => return Err(CliError::UnknownArgument { argument }),
        }
    }
    if result.timeout_ms == 0 {
        return Err(CliError::ZeroPacketTimeout);
    }
    if result.baud != DEFAULT_BAUD {
        return Err(CliError::UnsupportedBaud {
            observed: result.baud,
        });
    }
    Ok(result)
}
