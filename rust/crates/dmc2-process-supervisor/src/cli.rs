use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use crate::catalog::{self, ArgumentPlacement, ProcessRole};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub role: ProcessRole,
    pub journal: PathBuf,
    pub program: OsString,
    pub arguments: Vec<OsString>,
}

impl Invocation {
    pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self, CliError> {
        let arguments = arguments.into_iter().collect::<Vec<_>>();
        let mut index = 0;
        let mut forwarded_prefix = Vec::new();

        if arguments.get(index).is_some_and(|value| value == "-ini") {
            forwarded_prefix.push(OsString::from("-ini"));
            index += 1;
            let ini = arguments
                .get(index)
                .ok_or(CliError::MissingForwardedIniPath)?;
            if ini.is_empty() {
                return Err(CliError::EmptyForwardedIniPath);
            }
            forwarded_prefix.push(ini.clone());
            index += 1;
        }

        require_literal(arguments.get(index), "--role")?;
        index += 1;
        let role_argument = arguments.get(index).ok_or(CliError::MissingRole)?;
        let role = catalog::role(role_argument).map_err(CliError::Role)?;
        index += 1;

        match (forwarded_prefix.is_empty(), role.argument_placement()) {
            (true, ArgumentPlacement::LinuxCncPrependsIni) => {
                return Err(CliError::ForwardedIniRequired { role });
            }
            (false, ArgumentPlacement::LinuxCncPrependsIni) => {}
            (false, observed) => {
                return Err(CliError::ForwardedIniUnexpected { role, observed });
            }
            (true, _) => {}
        }

        require_literal(arguments.get(index), "--journal")?;
        index += 1;
        let journal = arguments.get(index).ok_or(CliError::MissingJournalPath)?;
        if journal.is_empty() {
            return Err(CliError::EmptyJournalPath);
        }
        index += 1;

        require_literal(arguments.get(index), "--")?;
        index += 1;
        let program = arguments.get(index).ok_or(CliError::MissingProgram)?;
        if program.is_empty() {
            return Err(CliError::EmptyProgram);
        }
        if program.as_bytes() != role.program().as_bytes() {
            return Err(CliError::ProgramMismatch {
                role,
                expected: role.program(),
                observed: program.clone(),
            });
        }
        index += 1;

        let mut child_arguments = forwarded_prefix;
        child_arguments.extend(arguments[index..].iter().cloned());
        Ok(Self {
            role,
            journal: PathBuf::from(journal),
            program: program.clone(),
            arguments: child_arguments,
        })
    }
}

fn require_literal(argument: Option<&OsString>, expected: &'static str) -> Result<(), CliError> {
    match argument {
        Some(argument) if argument == OsStr::new(expected) => Ok(()),
        Some(observed) => Err(CliError::ExpectedLiteral {
            expected,
            observed: observed.clone(),
        }),
        None => Err(CliError::ExpectedLiteral {
            expected,
            observed: OsString::new(),
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    ExpectedLiteral {
        expected: &'static str,
        observed: OsString,
    },
    MissingForwardedIniPath,
    EmptyForwardedIniPath,
    MissingRole,
    Role(catalog::CatalogError),
    ForwardedIniRequired {
        role: ProcessRole,
    },
    ForwardedIniUnexpected {
        role: ProcessRole,
        observed: ArgumentPlacement,
    },
    MissingJournalPath,
    EmptyJournalPath,
    MissingProgram,
    EmptyProgram,
    ProgramMismatch {
        role: ProcessRole,
        expected: &'static str,
        observed: OsString,
    },
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedLiteral { expected, observed } if observed.is_empty() => {
                write!(formatter, "missing {expected}; {}", usage())
            }
            Self::ExpectedLiteral { expected, observed } => {
                write!(
                    formatter,
                    "expected {expected}, observed {observed:?}; {}",
                    usage()
                )
            }
            Self::MissingForwardedIniPath => {
                write!(
                    formatter,
                    "LinuxCNC supplied -ini without a path; {}",
                    usage()
                )
            }
            Self::EmptyForwardedIniPath => {
                write!(
                    formatter,
                    "LinuxCNC supplied an empty -ini path; {}",
                    usage()
                )
            }
            Self::MissingRole => write!(formatter, "missing process role; {}", usage()),
            Self::Role(error) => write!(formatter, "invalid process role: {error}"),
            Self::ForwardedIniRequired { role } => write!(
                formatter,
                "role {} requires LinuxCNC's leading -ini PATH arguments",
                role.name()
            ),
            Self::ForwardedIniUnexpected { role, observed } => write!(
                formatter,
                "role {} uses argument placement {} and cannot accept a leading -ini PATH",
                role.name(),
                observed.name()
            ),
            Self::MissingJournalPath => write!(formatter, "missing lifecycle-journal path"),
            Self::EmptyJournalPath => write!(formatter, "lifecycle-journal path is empty"),
            Self::MissingProgram => write!(formatter, "missing supervised program"),
            Self::EmptyProgram => write!(formatter, "supervised program is empty"),
            Self::ProgramMismatch {
                role,
                expected,
                observed,
            } => write!(
                formatter,
                "role {} requires exact program {expected:?}, observed {observed:?}",
                role.name()
            ),
        }
    }
}

impl std::error::Error for CliError {}

fn usage() -> &'static str {
    "usage: [-ini PATH] --role ROLE --journal PATH -- PROGRAM [ARG ...]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_linuxcnc_task_contract() {
        let parsed = Invocation::parse(
            [
                "--role",
                "milltask",
                "--journal",
                "../var/log/linuxcnc/process-lifecycle.tsv",
                "--",
                "/usr/bin/milltask",
                "-ini",
                "dmc2.ini",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("valid invocation");

        assert_eq!(parsed.role.name(), "milltask");
        assert_eq!(
            parsed.journal,
            PathBuf::from("../var/log/linuxcnc/process-lifecycle.tsv")
        );
        assert_eq!(parsed.program, OsString::from("/usr/bin/milltask"));
        assert_eq!(
            parsed.arguments,
            [OsString::from("-ini"), OsString::from("dmc2.ini")]
        );
    }

    #[test]
    fn forwards_linuxcncs_display_ini_prefix_to_axis() {
        let parsed = Invocation::parse(
            [
                "-ini",
                "/project/live/dmc2.ini",
                "--role",
                "axis",
                "--journal",
                "events.tsv",
                "--",
                "/usr/bin/axis",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("valid AXIS invocation");

        assert_eq!(parsed.role.name(), "axis");
        assert_eq!(
            parsed.arguments,
            [
                OsString::from("-ini"),
                OsString::from("/project/live/dmc2.ini")
            ]
        );
    }

    #[test]
    fn refuses_a_program_that_does_not_match_the_role_catalog() {
        let error = Invocation::parse(
            [
                "--role",
                "milltask",
                "--journal",
                "events.tsv",
                "--",
                "/bin/true",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect_err("program mismatch must fail");

        assert!(matches!(error, CliError::ProgramMismatch { .. }));
    }

    #[test]
    fn refuses_an_implicit_program_boundary() {
        let error = Invocation::parse(
            [
                "--role",
                "milltask",
                "--journal",
                "events.tsv",
                "/usr/bin/milltask",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect_err("missing separator must fail");

        assert!(matches!(
            error,
            CliError::ExpectedLiteral { expected: "--", .. }
        ));
    }
}
