use std::env;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

use crate::catalog::default_catalog_path;
use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};

pub const USAGE: &str = "Usage: dmc2ctl [--catalog PATH] [--nml-file PATH] <list|describe ID|status|execute ID|load ID|run ID>";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    List,
    Describe(String),
    Status,
    Execute(String),
    Load(String),
    Run(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Arguments {
    pub catalog: PathBuf,
    pub nml_file: PathBuf,
    pub action: Action,
}

pub fn parse_environment() -> Result<Option<Arguments>, CliError> {
    parse(
        env::args_os().skip(1),
        default_catalog_path(),
        env::var_os("EMC2_NMLFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/share/linuxcnc/linuxcnc.nml")),
    )
}

fn parse(
    values: impl IntoIterator<Item = OsString>,
    default_catalog: PathBuf,
    default_nml_file: PathBuf,
) -> Result<Option<Arguments>, CliError> {
    let values = values.into_iter().collect::<Vec<_>>();
    if values.len() == 1 && matches!(values[0].to_str(), Some("--help" | "-h")) {
        return Ok(None);
    }

    let mut catalog = default_catalog;
    let mut nml_file = default_nml_file;
    let mut index = 0;
    while let Some(option) = values.get(index).and_then(|value| value.to_str()) {
        match option {
            "--catalog" => {
                catalog = PathBuf::from(value(&values, index, option)?);
                index += 2;
            }
            "--nml-file" => {
                nml_file = PathBuf::from(value(&values, index, option)?);
                index += 2;
            }
            _ => break,
        }
    }
    let command = values
        .get(index)
        .ok_or(CliError::MissingCommand)?
        .to_str()
        .ok_or_else(|| CliError::NonUtf8Command(values[index].clone()))?;
    let remaining = &values[index + 1..];
    let action = match command {
        "list" if remaining.is_empty() => Action::List,
        "status" if remaining.is_empty() => Action::Status,
        "describe" => Action::Describe(single_id(command, remaining)?),
        "execute" => Action::Execute(single_id(command, remaining)?),
        "load" => Action::Load(single_id(command, remaining)?),
        "run" => Action::Run(single_id(command, remaining)?),
        "list" | "status" => return Err(CliError::UnexpectedArguments(command.to_owned())),
        _ => return Err(CliError::UnknownCommand(command.to_owned())),
    };
    Ok(Some(Arguments {
        catalog,
        nml_file,
        action,
    }))
}

fn value(values: &[OsString], index: usize, option: &str) -> Result<OsString, CliError> {
    values
        .get(index + 1)
        .cloned()
        .ok_or_else(|| CliError::MissingOptionValue(option.to_owned()))
}

fn single_id(command: &str, remaining: &[OsString]) -> Result<String, CliError> {
    if remaining.len() != 1 {
        return Err(CliError::ExpectedOneId(command.to_owned()));
    }
    remaining[0]
        .clone()
        .into_string()
        .map_err(CliError::NonUtf8Id)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliError {
    MissingCommand,
    MissingOptionValue(String),
    NonUtf8Command(OsString),
    NonUtf8Id(OsString),
    UnknownCommand(String),
    UnexpectedArguments(String),
    ExpectedOneId(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCommand => formatter.write_str("missing command"),
            Self::MissingOptionValue(option) => write!(formatter, "{option} requires a path"),
            Self::NonUtf8Command(value) => write!(formatter, "command is not UTF-8: {value:?}"),
            Self::NonUtf8Id(value) => write!(formatter, "operation ID is not UTF-8: {value:?}"),
            Self::UnknownCommand(value) => write!(formatter, "unknown command {value:?}"),
            Self::UnexpectedArguments(value) => {
                write!(formatter, "{value} does not accept arguments")
            }
            Self::ExpectedOneId(value) => write!(formatter, "{value} requires exactly one ID"),
        }
    }
}

impl RecoveryClassified for CliError {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::MissingCommand
            | Self::MissingOptionValue(_)
            | Self::NonUtf8Command(_)
            | Self::NonUtf8Id(_)
            | Self::UnknownCommand(_)
            | Self::UnexpectedArguments(_)
            | Self::ExpectedOneId(_) => RecoveryClass::RelaunchApplication,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_and_run_are_distinct_commands() {
        let base = PathBuf::from("catalog.tsv");
        let nml = PathBuf::from("linuxcnc.nml");
        let load = parse(
            [OsString::from("load"), OsString::from("program.test")],
            base.clone(),
            nml.clone(),
        )
        .expect("load should parse")
        .expect("load is not help");
        let run = parse(
            [OsString::from("run"), OsString::from("program.test")],
            base,
            nml,
        )
        .expect("run should parse")
        .expect("run is not help");
        assert_eq!(load.action, Action::Load("program.test".to_owned()));
        assert_eq!(run.action, Action::Run("program.test".to_owned()));
    }
}
