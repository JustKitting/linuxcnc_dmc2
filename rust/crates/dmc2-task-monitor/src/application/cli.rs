use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::PathBuf;

const DEFAULT_COMPONENT: &str = "dmc2-task-monitor";
const DEFAULT_NML_FILE: &str = "/usr/share/linuxcnc/linuxcnc.nml";
const USAGE: &str = "Usage: dmc2-task-monitor [--component NAME] [--nml-file PATH] --error-journal PATH --diagnostic-journal PATH";

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Arguments {
    pub(super) component: String,
    pub(super) nml_file: OsString,
    pub(super) error_journal: Option<PathBuf>,
    pub(super) diagnostic_journal: Option<PathBuf>,
}

#[derive(Debug, Eq, PartialEq)]
enum ParseOutcome {
    Run(Arguments),
    Help,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CliOption {
    Component,
    NmlFile,
    ErrorJournal,
    DiagnosticJournal,
}

impl CliOption {
    const fn name(self) -> &'static str {
        match self {
            Self::Component => "--component",
            Self::NmlFile => "--nml-file",
            Self::ErrorJournal => "--error-journal",
            Self::DiagnosticJournal => "--diagnostic-journal",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CliError {
    HelpCombined,
    NonUtf8Argument { index: usize, value: OsString },
    MissingValue { option: CliOption },
    DuplicateOption { option: CliOption },
    UnknownArgument { argument: String },
    RequiredOptionMissing { option: CliOption },
    NonUtf8Value { option: CliOption, value: OsString },
    EmptyValue { option: CliOption },
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HelpCombined => formatter.write_str("--help must be the only argument"),
            Self::NonUtf8Argument { index, value } => write!(
                formatter,
                "TASK_MONITOR_ARGUMENT_NOT_UTF8: index={} value={value:?}; action: pass a UTF-8 option name",
                index + 1
            ),
            Self::MissingValue { option } => {
                write!(formatter, "{} requires a value", option.name())
            }
            Self::DuplicateOption { option } => {
                write!(formatter, "{} was provided more than once", option.name())
            }
            Self::UnknownArgument { argument } => write!(formatter, "unknown argument: {argument}"),
            Self::RequiredOptionMissing { option } => {
                write!(formatter, "runtime mode requires {} PATH", option.name())
            }
            Self::NonUtf8Value { option, value } => write!(
                formatter,
                "TASK_MONITOR_OPTION_VALUE_NOT_UTF8: option={} value={value:?}; action: pass a UTF-8 component name",
                option.name()
            ),
            Self::EmptyValue { option } => {
                write!(formatter, "{} value cannot be empty", option.name())
            }
        }
    }
}

pub(super) fn arguments() -> Result<Arguments, CliError> {
    let nml_file = env::var_os("EMC2_NMLFILE").unwrap_or_else(|| OsString::from(DEFAULT_NML_FILE));
    match parse_arguments(env::args_os().skip(1), nml_file)? {
        ParseOutcome::Run(arguments) => Ok(arguments),
        ParseOutcome::Help => {
            println!("{USAGE}");
            std::process::exit(0);
        }
    }
}

fn parse_arguments(
    items: impl IntoIterator<Item = OsString>,
    default_nml_file: OsString,
) -> Result<ParseOutcome, CliError> {
    let items = items.into_iter().collect::<Vec<_>>();
    if items.len() == 1 && matches!(items[0].to_str(), Some("--help" | "-h")) {
        return Ok(ParseOutcome::Help);
    }
    if items
        .iter()
        .any(|item| matches!(item.to_str(), Some("--help" | "-h")))
    {
        return Err(CliError::HelpCombined);
    }

    let mut component = None;
    let mut nml_file = None;
    let mut error_journal = None;
    let mut diagnostic_journal = None;
    let mut index = 0;
    while index < items.len() {
        let argument = items[index]
            .to_str()
            .ok_or_else(|| CliError::NonUtf8Argument {
                index,
                value: items[index].clone(),
            })?;
        match argument {
            "--component" => {
                set_once(
                    &mut component,
                    value(&items, index, CliOption::Component)?,
                    CliOption::Component,
                )?;
                index += 2;
            }
            "--nml-file" => {
                set_once(
                    &mut nml_file,
                    value(&items, index, CliOption::NmlFile)?,
                    CliOption::NmlFile,
                )?;
                index += 2;
            }
            "--error-journal" => {
                set_once(
                    &mut error_journal,
                    value(&items, index, CliOption::ErrorJournal)?,
                    CliOption::ErrorJournal,
                )?;
                index += 2;
            }
            "--diagnostic-journal" => {
                set_once(
                    &mut diagnostic_journal,
                    value(&items, index, CliOption::DiagnosticJournal)?,
                    CliOption::DiagnosticJournal,
                )?;
                index += 2;
            }
            _ => {
                return Err(CliError::UnknownArgument {
                    argument: argument.to_owned(),
                })
            }
        }
    }

    if error_journal.is_none() {
        return Err(CliError::RequiredOptionMissing {
            option: CliOption::ErrorJournal,
        });
    }
    if diagnostic_journal.is_none() {
        return Err(CliError::RequiredOptionMissing {
            option: CliOption::DiagnosticJournal,
        });
    }
    let component = match component {
        Some(value) => value
            .into_string()
            .map_err(|value| CliError::NonUtf8Value {
                option: CliOption::Component,
                value,
            })?,
        None => DEFAULT_COMPONENT.to_owned(),
    };
    if component.is_empty() {
        return Err(CliError::EmptyValue {
            option: CliOption::Component,
        });
    }
    let nml_file = nml_file.unwrap_or(default_nml_file);
    if os_str_bytes(&nml_file).is_empty() {
        return Err(CliError::EmptyValue {
            option: CliOption::NmlFile,
        });
    }
    let error_journal = error_journal.map(PathBuf::from);
    if error_journal
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(CliError::EmptyValue {
            option: CliOption::ErrorJournal,
        });
    }
    let diagnostic_journal = diagnostic_journal.map(PathBuf::from);
    if diagnostic_journal
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(CliError::EmptyValue {
            option: CliOption::DiagnosticJournal,
        });
    }

    Ok(ParseOutcome::Run(Arguments {
        component,
        nml_file,
        error_journal,
        diagnostic_journal,
    }))
}

fn value(items: &[OsString], index: usize, option: CliOption) -> Result<OsString, CliError> {
    items
        .get(index + 1)
        .cloned()
        .ok_or(CliError::MissingValue { option })
}

fn set_once<T>(slot: &mut Option<T>, value: T, option: CliOption) -> Result<(), CliError> {
    if slot.replace(value).is_some() {
        return Err(CliError::DuplicateOption { option });
    }
    Ok(())
}

#[cfg(unix)]
fn os_str_bytes(value: &OsStr) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes()
}
