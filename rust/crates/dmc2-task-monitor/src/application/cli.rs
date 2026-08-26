use std::env;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

const DEFAULT_COMPONENT: &str = "dmc2-task-monitor";
const DEFAULT_NML_FILE: &str = "/usr/share/linuxcnc/linuxcnc.nml";
const USAGE: &str = "Usage: dmc2-task-monitor [--component NAME] [--nml-file PATH] --error-journal PATH\n       dmc2-task-monitor (--validate|--validate-json)";

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Arguments {
    pub(super) component: String,
    pub(super) nml_file: OsString,
    pub(super) error_journal: Option<PathBuf>,
    pub(super) validate: bool,
    pub(super) validation_json: bool,
}

#[derive(Debug, Eq, PartialEq)]
enum ParseOutcome {
    Run(Arguments),
    Help,
}

pub(super) fn arguments() -> Result<Arguments, String> {
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
) -> Result<ParseOutcome, String> {
    let items = items.into_iter().collect::<Vec<_>>();
    if items.len() == 1 && matches!(items[0].to_str(), Some("--help" | "-h")) {
        return Ok(ParseOutcome::Help);
    }
    if items
        .iter()
        .any(|item| matches!(item.to_str(), Some("--help" | "-h")))
    {
        return Err("--help must be the only argument".to_owned());
    }

    let mut component = None;
    let mut nml_file = None;
    let mut error_journal = None;
    let mut validation = None;
    let mut index = 0;
    while index < items.len() {
        let argument = items[index]
            .to_str()
            .ok_or_else(|| format!("argument {} is not valid UTF-8", index + 1))?;
        match argument {
            "--component" => {
                set_once(&mut component, value(&items, index, argument)?, argument)?;
                index += 2;
            }
            "--nml-file" => {
                set_once(&mut nml_file, value(&items, index, argument)?, argument)?;
                index += 2;
            }
            "--error-journal" => {
                set_once(
                    &mut error_journal,
                    value(&items, index, argument)?,
                    argument,
                )?;
                index += 2;
            }
            "--validate" | "--validate-json" => {
                set_once(&mut validation, argument.to_owned(), "validation mode")?;
                index += 1;
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }

    let validation_json = validation.as_deref() == Some("--validate-json");
    let validate = validation.is_some();
    if validate && (component.is_some() || nml_file.is_some() || error_journal.is_some()) {
        return Err("validation mode cannot be combined with runtime options".to_owned());
    }
    if !validate && error_journal.is_none() {
        return Err("runtime mode requires --error-journal PATH".to_owned());
    }
    let component = match component {
        Some(value) => value
            .into_string()
            .map_err(|_| "--component value is not valid UTF-8".to_owned())?,
        None => DEFAULT_COMPONENT.to_owned(),
    };
    if component.is_empty() {
        return Err("--component value cannot be empty".to_owned());
    }
    let nml_file = nml_file.unwrap_or(default_nml_file);
    if os_str_bytes(&nml_file).is_empty() {
        return Err("--nml-file value cannot be empty".to_owned());
    }
    let error_journal = error_journal.map(PathBuf::from);
    if error_journal
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err("--error-journal value cannot be empty".to_owned());
    }

    Ok(ParseOutcome::Run(Arguments {
        component,
        nml_file,
        error_journal,
        validate,
        validation_json,
    }))
}

fn value(items: &[OsString], index: usize, argument: &str) -> Result<OsString, String> {
    items
        .get(index + 1)
        .cloned()
        .ok_or_else(|| format!("{argument} requires a value"))
}

fn set_once<T>(slot: &mut Option<T>, value: T, option: &str) -> Result<(), String> {
    if slot.replace(value).is_some() {
        return Err(format!("{option} was provided more than once"));
    }
    Ok(())
}

#[cfg(unix)]
fn os_str_bytes(value: &OsStr) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes()
}

#[cfg(test)]
mod tests {
    use std::os::unix::ffi::OsStringExt;

    use super::*;

    fn parse(arguments: &[&str]) -> Result<ParseOutcome, String> {
        parse_arguments(
            arguments.iter().map(OsString::from),
            OsString::from("default.nml"),
        )
    }

    #[test]
    fn validation_help_and_runtime_have_exact_disjoint_forms() {
        assert_eq!(parse(&["--help"]).unwrap(), ParseOutcome::Help);
        assert_eq!(parse(&["-h"]).unwrap(), ParseOutcome::Help);
        assert_eq!(
            parse(&["--validate"]).unwrap(),
            ParseOutcome::Run(Arguments {
                component: DEFAULT_COMPONENT.to_owned(),
                nml_file: OsString::from("default.nml"),
                error_journal: None,
                validate: true,
                validation_json: false,
            })
        );
        assert_eq!(
            parse(&["--validate-json"]).unwrap(),
            ParseOutcome::Run(Arguments {
                component: DEFAULT_COMPONENT.to_owned(),
                nml_file: OsString::from("default.nml"),
                error_journal: None,
                validate: true,
                validation_json: true,
            })
        );
        assert_eq!(
            parse(&[
                "--component",
                "monitor",
                "--nml-file",
                "machine.nml",
                "--error-journal",
                "errors.tsv",
            ])
            .unwrap(),
            ParseOutcome::Run(Arguments {
                component: "monitor".to_owned(),
                nml_file: OsString::from("machine.nml"),
                error_journal: Some(PathBuf::from("errors.tsv")),
                validate: false,
                validation_json: false,
            })
        );
    }

    #[test]
    fn missing_duplicate_combined_and_unknown_options_fail_closed() {
        for arguments in [
            vec![],
            vec!["--component"],
            vec!["--nml-file"],
            vec!["--error-journal"],
            vec!["--help", "--validate"],
            vec!["--validate", "--validate-json"],
            vec!["--validate", "--component", "x"],
            vec!["--error-journal", "a", "--error-journal", "b"],
            vec!["--unknown"],
        ] {
            assert!(parse(&arguments).is_err(), "accepted {arguments:?}");
        }
    }

    #[test]
    fn path_bytes_are_preserved_and_non_utf8_option_or_component_is_rejected() {
        let non_utf8 = OsString::from_vec(vec![b'p', 0xff]);
        let parsed = parse_arguments(
            [
                OsString::from("--nml-file"),
                non_utf8.clone(),
                OsString::from("--error-journal"),
                non_utf8.clone(),
            ],
            OsString::from("default.nml"),
        )
        .unwrap();
        let ParseOutcome::Run(arguments) = parsed else {
            panic!("runtime arguments parsed as help");
        };
        assert_eq!(arguments.nml_file, non_utf8);
        assert_eq!(arguments.error_journal, Some(PathBuf::from(non_utf8)));

        assert!(parse_arguments(
            [OsString::from_vec(vec![b'-', 0xff])],
            OsString::from("default.nml")
        )
        .is_err());
        assert!(parse_arguments(
            [
                OsString::from("--component"),
                OsString::from_vec(vec![0xff]),
                OsString::from("--error-journal"),
                OsString::from("errors.tsv"),
            ],
            OsString::from("default.nml"),
        )
        .is_err());
    }
}
