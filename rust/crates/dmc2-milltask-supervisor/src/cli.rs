use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub journal: PathBuf,
    pub program: OsString,
    pub arguments: Vec<OsString>,
}

impl Invocation {
    pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self, CliError> {
        let mut arguments = arguments.into_iter();
        require_literal(arguments.next(), "--journal")?;

        let journal = arguments.next().ok_or(CliError::MissingJournalPath)?;
        if journal.is_empty() {
            return Err(CliError::EmptyJournalPath);
        }

        require_literal(arguments.next(), "--")?;
        let program = arguments.next().ok_or(CliError::MissingProgram)?;
        if program.is_empty() {
            return Err(CliError::EmptyProgram);
        }

        Ok(Self {
            journal: PathBuf::from(journal),
            program,
            arguments: arguments.collect(),
        })
    }
}

fn require_literal(argument: Option<OsString>, expected: &'static str) -> Result<(), CliError> {
    match argument {
        Some(argument) if argument == OsStr::new(expected) => Ok(()),
        Some(observed) => Err(CliError::ExpectedLiteral { expected, observed }),
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
    MissingJournalPath,
    EmptyJournalPath,
    MissingProgram,
    EmptyProgram,
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedLiteral { expected, observed } if observed.is_empty() => write!(
                formatter,
                "missing {expected}; usage: --journal PATH -- PROGRAM [ARG ...]"
            ),
            Self::ExpectedLiteral { expected, observed } => write!(
                formatter,
                "expected {expected}, observed {observed:?}; usage: --journal PATH -- PROGRAM [ARG ...]"
            ),
            Self::MissingJournalPath => write!(formatter, "missing lifecycle-journal path"),
            Self::EmptyJournalPath => write!(formatter, "lifecycle-journal path is empty"),
            Self::MissingProgram => write!(formatter, "missing supervised program"),
            Self::EmptyProgram => write!(formatter, "supervised program is empty"),
        }
    }
}

impl std::error::Error for CliError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_linuxcnc_task_contract() {
        let parsed = Invocation::parse(
            [
                "--journal",
                "../var/log/linuxcnc/milltask-lifecycle.tsv",
                "--",
                "/usr/bin/milltask",
                "-ini",
                "dmc2.ini",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("valid invocation");

        assert_eq!(
            parsed.journal,
            PathBuf::from("../var/log/linuxcnc/milltask-lifecycle.tsv")
        );
        assert_eq!(parsed.program, OsString::from("/usr/bin/milltask"));
        assert_eq!(
            parsed.arguments,
            [OsString::from("-ini"), OsString::from("dmc2.ini")]
        );
    }

    #[test]
    fn refuses_an_implicit_program_boundary() {
        let error = Invocation::parse(
            ["--journal", "events.tsv", "/usr/bin/milltask"]
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
