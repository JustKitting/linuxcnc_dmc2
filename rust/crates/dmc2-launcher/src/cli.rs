use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;

use crate::error::{render_bytes, Error};

pub const USAGE: &str = "usage: dmc2-linuxcnc [--live [--persistent]]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Help,
    Launch(Mode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Validate,
    Direct,
    Persistent,
}

pub fn parse(arguments: &[OsString]) -> Result<Command, Error> {
    let mut live = false;
    let mut persistent = false;
    let mut help = false;

    for argument in arguments {
        let Some(argument) = argument.to_str() else {
            return Err(Error::Usage(format!(
                "{USAGE}; argument is not valid UTF-8: {}",
                render_bytes(argument.as_os_str().as_bytes())
            )));
        };
        match argument {
            "--live" if !live => live = true,
            "--persistent" if !persistent => persistent = true,
            "--help" | "-h" if !help => help = true,
            "--live" | "--persistent" | "--help" | "-h" => {
                return Err(Error::Usage(format!(
                    "{USAGE}; duplicate argument {argument}"
                )));
            }
            _ => {
                return Err(Error::Usage(format!(
                    "{USAGE}; unsupported argument {argument:?}"
                )));
            }
        }
    }

    if help {
        if live || persistent {
            return Err(Error::Usage(format!(
                "{USAGE}; help cannot be combined with live options"
            )));
        }
        return Ok(Command::Help);
    }
    if persistent && !live {
        return Err(Error::Usage(format!(
            "{USAGE}; --persistent requires --live"
        )));
    }
    let mode = if !live {
        Mode::Validate
    } else if persistent {
        Mode::Persistent
    } else {
        Mode::Direct
    };
    Ok(Command::Launch(mode))
}
