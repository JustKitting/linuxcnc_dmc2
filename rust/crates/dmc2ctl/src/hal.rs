use std::fmt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::dispatch::STATUS_POLL_PERIOD;
use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};

pub const PHYSICAL_PENDANT_ESTOP_PIN: &str = "dmc2-pendant.estop-pressed";
pub const CLEAR_REQUEST_PIN: &str = "dmc2-pendant-control.clear-fault-request";
pub const CLEAR_ACK_PIN: &str = "dmc2-pendant-control.clear-fault-ack";
// Same deadline as the native LinuxCNC command client.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug)]
pub enum DriverReset {
    MesaIoError,
    MesaWatchdog,
}

#[derive(Clone, Copy, Debug)]
pub enum HalCommand {
    Read(&'static str),
    ClearController(u32),
    AcknowledgeDriver(DriverReset),
}

impl fmt::Display for HalCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(pin) => write!(f, "read {pin}"),
            Self::ClearController(request) => write!(f, "submit Clear Fault request {request}"),
            Self::AcknowledgeDriver(DriverReset::MesaIoError) => {
                f.write_str("acknowledge the retained Mesa communication error")
            }
            Self::AcknowledgeDriver(DriverReset::MesaWatchdog) => {
                f.write_str("acknowledge the Mesa watchdog")
            }
        }
    }
}

fn execute(command: HalCommand) -> Result<String, HalError> {
    let mut process = Command::new("halcmd");
    process.env("LINUXCNC_FORCE_REALTIME", "1");
    match command {
        HalCommand::Read(pin) => {
            process.args(["-s", "getp", pin]);
        }
        HalCommand::ClearController(request) => {
            process.args(["setp", CLEAR_REQUEST_PIN, &request.to_string()]);
        }
        HalCommand::AcknowledgeDriver(DriverReset::MesaIoError) => {
            process.args(["setp", "hm2_7i95.0.io_error", "FALSE"]);
        }
        HalCommand::AcknowledgeDriver(DriverReset::MesaWatchdog) => {
            // Linked HAL_IO pin: acknowledge its signal, not the linked pin.
            process.args(["sets", "dmc2-mesa-watchdog-fault", "FALSE"]);
        }
    }
    let mut child = process
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| HalError::Io { command, source })?;
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    loop {
        if child
            .try_wait()
            .map_err(|source| HalError::Io { command, source })?
            .is_some()
        {
            break;
        }
        if Instant::now() >= deadline {
            // Only this invocation's HAL helper is ended; never LinuxCNC.
            child
                .kill()
                .map_err(|source| HalError::Io { command, source })?;
            child
                .wait()
                .map_err(|source| HalError::Io { command, source })?;
            return Err(HalError::Timeout(command));
        }
        thread::sleep(STATUS_POLL_PERIOD);
    }
    let output = child
        .wait_with_output()
        .map_err(|source| HalError::Io { command, source })?;
    if !output.status.success() {
        return Err(HalError::Command {
            command,
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    String::from_utf8(output.stdout).map_err(|source| HalError::NonUtf8 { command, source })
}

pub fn acknowledge_driver(reset: DriverReset) -> Result<(), HalError> {
    execute(HalCommand::AcknowledgeDriver(reset)).map(|_| ())
}

pub fn submit_clear() -> Result<u32, HalError> {
    let request = read_u32(CLEAR_REQUEST_PIN)?.wrapping_add(1).max(1);
    execute(HalCommand::ClearController(request))?;
    Ok(request)
}

pub fn read_u32(pin: &'static str) -> Result<u32, HalError> {
    let value = execute(HalCommand::Read(pin))?;
    let value = value.trim();
    let parsed = match value.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => value.parse(),
    };
    parsed.map_err(|_| HalError::InvalidValue {
        pin,
        expected: "unsigned request number",
        observed: value.to_owned(),
    })
}

pub fn read_bit(pin: &'static str) -> Result<bool, HalError> {
    let value = execute(HalCommand::Read(pin))?;
    match value.trim() {
        "TRUE" | "1" => Ok(true),
        "FALSE" | "0" => Ok(false),
        observed => Err(HalError::InvalidValue {
            pin,
            expected: "TRUE or FALSE",
            observed: observed.to_owned(),
        }),
    }
}

#[derive(Debug)]
pub enum HalError {
    Io {
        command: HalCommand,
        source: std::io::Error,
    },
    Command {
        command: HalCommand,
        status: Option<i32>,
        stderr: String,
    },
    Timeout(HalCommand),
    NonUtf8 {
        command: HalCommand,
        source: std::string::FromUtf8Error,
    },
    InvalidValue {
        pin: &'static str,
        expected: &'static str,
        observed: String,
    },
}

impl fmt::Display for HalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { command, source } => write!(f, "Cannot {command}: {source}. Clear Fault remains available to retry."),
            Self::Command { command, status, stderr } => write!(f, "Cannot {command}: {stderr} (exit {status:?}). Retry Clear Fault; if the controller component is absent, use the CNC launcher to reopen the session."),
            Self::Timeout(command) => write!(f, "HAL did not respond while trying to {command}. This attempt has ended; retry Clear Fault or reopen the session through the CNC launcher if HAL remains unresponsive."),
            Self::NonUtf8 { command, source } => write!(f, "Invalid HAL reply while trying to {command}: {source}. Retry Clear Fault or reopen the session through the CNC launcher."),
            Self::InvalidValue { pin, expected, observed } => write!(f, "HAL input {pin} reported {observed:?}; expected {expected}. Retry Clear Fault or reopen the session through the CNC launcher."),
        }
    }
}

impl RecoveryClassified for HalError {
    fn recovery_class(&self) -> RecoveryClass {
        RecoveryClass::ClearController
    }
}
