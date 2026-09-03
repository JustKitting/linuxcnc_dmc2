use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

const CATALOG: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../config/processes.tsv"
));
const MAGIC: &str = "DMC2_PROCESS_CATALOG\t1";
const HEADER: &str =
    "role\tprogram\tlaunch_site\townership\tcriticality\tbacktrace\targument_placement";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessRole {
    definition: ProcessDefinition,
}

impl ProcessRole {
    pub const fn name(self) -> &'static str {
        self.definition.role
    }

    pub const fn program(self) -> &'static str {
        self.definition.program
    }

    pub const fn launch_site(self) -> &'static str {
        self.definition.launch_site
    }

    pub const fn ownership(self) -> Ownership {
        self.definition.ownership
    }

    pub const fn criticality(self) -> Criticality {
        self.definition.criticality
    }

    pub const fn backtrace(self) -> BacktraceKind {
        self.definition.backtrace
    }

    pub const fn argument_placement(self) -> ArgumentPlacement {
        self.definition.argument_placement
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProcessDefinition {
    role: &'static str,
    program: &'static str,
    launch_site: &'static str,
    ownership: Ownership,
    criticality: Criticality,
    backtrace: BacktraceKind,
    argument_placement: ArgumentPlacement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    DirectChild,
    SessionRoot,
    SelfDaemonizingDescendant,
    PersistentMaster,
}

impl Ownership {
    pub const fn name(self) -> &'static str {
        match self {
            Self::DirectChild => "direct-child",
            Self::SessionRoot => "session-root",
            Self::SelfDaemonizingDescendant => "self-daemonizing-descendant",
            Self::PersistentMaster => "persistent-master",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Criticality {
    ControllerCritical,
    OperatorInterface,
    VerificationOnly,
}

impl Criticality {
    pub const fn name(self) -> &'static str {
        match self {
            Self::ControllerCritical => "controller-critical",
            Self::OperatorInterface => "operator-interface",
            Self::VerificationOnly => "verification-only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacktraceKind {
    None,
    LinuxCncTask,
}

impl BacktraceKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::LinuxCncTask => "linuxcnc-task",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgumentPlacement {
    None,
    LinuxCncAppendsIni,
    LinuxCncPrependsIni,
}

impl ArgumentPlacement {
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::LinuxCncAppendsIni => "linuxcnc-appends-ini",
            Self::LinuxCncPrependsIni => "linuxcnc-prepends-ini",
        }
    }
}

pub fn role(value: &OsStr) -> Result<ProcessRole, CatalogError> {
    let requested = value
        .to_str()
        .ok_or_else(|| CatalogError::NonUtf8Role(value.to_os_string()))?;
    definitions()?
        .find(|definition| definition.role == requested)
        .map(|definition| ProcessRole { definition })
        .ok_or_else(|| CatalogError::UnknownRole(requested.to_owned()))
}

pub fn identify_process(
    executable: Option<&Path>,
    cmdline: &[u8],
    comm: Option<&[u8]>,
) -> Result<Option<(ProcessRole, IdentitySource)>, CatalogError> {
    let arguments = cmdline
        .split(|byte| *byte == 0)
        .filter(|argument| !argument.is_empty())
        .collect::<Vec<_>>();
    let comm = comm.map(trim_ascii_whitespace);
    for definition in
        definitions()?.filter(|item| item.criticality != Criticality::VerificationOnly)
    {
        let expected = Path::new(definition.program);
        let executable_match = executable.is_some_and(|observed| {
            if expected.is_absolute() {
                observed.as_os_str().as_bytes() == expected.as_os_str().as_bytes()
            } else {
                observed.file_name() == expected.file_name()
            }
        });
        if executable_match {
            return Ok(Some((
                ProcessRole { definition },
                IdentitySource::Executable,
            )));
        }
        if arguments
            .iter()
            .any(|argument| *argument == definition.program.as_bytes())
        {
            return Ok(Some((
                ProcessRole { definition },
                IdentitySource::CommandLine,
            )));
        }
        let expected_comm = expected
            .file_name()
            .map(OsStr::as_bytes)
            .unwrap_or_default();
        let expected_comm = &expected_comm[..expected_comm.len().min(15)];
        if comm.is_some_and(|observed| observed == expected_comm) {
            return Ok(Some((
                ProcessRole { definition },
                IdentitySource::ProcessComm,
            )));
        }
    }
    Ok(None)
}

fn trim_ascii_whitespace(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentitySource {
    Executable,
    CommandLine,
    ProcessComm,
}

impl IdentitySource {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Executable => "proc-executable",
            Self::CommandLine => "proc-command-line",
            Self::ProcessComm => "proc-comm",
        }
    }
}

fn definitions() -> Result<impl Iterator<Item = ProcessDefinition>, CatalogError> {
    let mut lines = CATALOG.lines();
    let magic = lines.next().unwrap_or_default();
    if magic != MAGIC {
        return Err(CatalogError::Magic(magic.to_owned()));
    }
    let header = lines.next().unwrap_or_default();
    if header != HEADER {
        return Err(CatalogError::Header(header.to_owned()));
    }
    Ok(lines
        .enumerate()
        .filter(|(_, line)| !line.is_empty())
        .map(|(index, line)| parse_definition(index + 3, line))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter())
}

fn parse_definition(
    line_number: usize,
    line: &'static str,
) -> Result<ProcessDefinition, CatalogError> {
    let fields = line.split('\t').collect::<Vec<_>>();
    if fields.len() != 7 {
        return Err(CatalogError::FieldCount {
            line: line_number,
            observed: fields.len(),
        });
    }
    let [role, program, launch_site, ownership, criticality, backtrace, argument_placement] =
        fields.as_slice()
    else {
        unreachable!("field count checked above")
    };
    for (name, value) in [
        ("role", *role),
        ("program", *program),
        ("launch_site", *launch_site),
    ] {
        if value.is_empty() {
            return Err(CatalogError::EmptyField {
                line: line_number,
                field: name,
            });
        }
    }
    Ok(ProcessDefinition {
        role,
        program,
        launch_site,
        ownership: parse_ownership(line_number, ownership)?,
        criticality: parse_criticality(line_number, criticality)?,
        backtrace: parse_backtrace(line_number, backtrace)?,
        argument_placement: parse_argument_placement(line_number, argument_placement)?,
    })
}

fn parse_ownership(line: usize, value: &str) -> Result<Ownership, CatalogError> {
    match value {
        "direct-child" => Ok(Ownership::DirectChild),
        "session-root" => Ok(Ownership::SessionRoot),
        "self-daemonizing-descendant" => Ok(Ownership::SelfDaemonizingDescendant),
        "persistent-master" => Ok(Ownership::PersistentMaster),
        _ => Err(CatalogError::Value {
            line,
            field: "ownership",
            value: value.to_owned(),
        }),
    }
}

fn parse_criticality(line: usize, value: &str) -> Result<Criticality, CatalogError> {
    match value {
        "controller-critical" => Ok(Criticality::ControllerCritical),
        "operator-interface" => Ok(Criticality::OperatorInterface),
        "verification-only" => Ok(Criticality::VerificationOnly),
        _ => Err(CatalogError::Value {
            line,
            field: "criticality",
            value: value.to_owned(),
        }),
    }
}

fn parse_backtrace(line: usize, value: &str) -> Result<BacktraceKind, CatalogError> {
    match value {
        "none" => Ok(BacktraceKind::None),
        "linuxcnc-task" => Ok(BacktraceKind::LinuxCncTask),
        _ => Err(CatalogError::Value {
            line,
            field: "backtrace",
            value: value.to_owned(),
        }),
    }
}

fn parse_argument_placement(line: usize, value: &str) -> Result<ArgumentPlacement, CatalogError> {
    match value {
        "none" => Ok(ArgumentPlacement::None),
        "linuxcnc-appends-ini" => Ok(ArgumentPlacement::LinuxCncAppendsIni),
        "linuxcnc-prepends-ini" => Ok(ArgumentPlacement::LinuxCncPrependsIni),
        _ => Err(CatalogError::Value {
            line,
            field: "argument_placement",
            value: value.to_owned(),
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    Magic(String),
    Header(String),
    FieldCount {
        line: usize,
        observed: usize,
    },
    EmptyField {
        line: usize,
        field: &'static str,
    },
    Value {
        line: usize,
        field: &'static str,
        value: String,
    },
    NonUtf8Role(OsString),
    UnknownRole(String),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Magic(observed) => write!(
                formatter,
                "process catalog magic/version mismatch: observed {observed:?}"
            ),
            Self::Header(observed) => {
                write!(
                    formatter,
                    "process catalog header mismatch: observed {observed:?}"
                )
            }
            Self::FieldCount { line, observed } => write!(
                formatter,
                "process catalog line {line} has {observed} fields; expected 7"
            ),
            Self::EmptyField { line, field } => {
                write!(formatter, "process catalog line {line} has empty {field}")
            }
            Self::Value { line, field, value } => write!(
                formatter,
                "process catalog line {line} has unknown {field} value {value:?}"
            ),
            Self::NonUtf8Role(value) => write!(formatter, "process role is not UTF-8: {value:?}"),
            Self::UnknownRole(value) => {
                write!(formatter, "process role is not catalogued: {value:?}")
            }
        }
    }
}

impl std::error::Error for CatalogError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn catalog_is_complete_and_has_unique_roles() {
        let definitions = definitions()
            .expect("valid process catalog")
            .collect::<Vec<_>>();
        let roles = definitions
            .iter()
            .map(|definition| definition.role)
            .collect::<BTreeSet<_>>();
        assert_eq!(roles.len(), definitions.len());
        assert_eq!(
            roles,
            BTreeSet::from([
                "axis",
                "halui",
                "iocontrol",
                "linuxcncsvr",
                "lifecycle-test",
                "linuxcnc-session",
                "milltask",
                "rtapi-app",
                "serial-bridge",
                "session-lifecycle-test",
                "task-monitor",
            ])
        );
    }

    #[test]
    fn milltask_contract_is_data_driven() {
        let role = role(OsStr::new("milltask")).expect("milltask role");
        assert_eq!(role.program(), "/usr/bin/milltask");
        assert_eq!(role.ownership(), Ownership::DirectChild);
        assert_eq!(role.backtrace(), BacktraceKind::LinuxCncTask);
    }

    #[test]
    fn identifies_script_roles_from_their_command_line() {
        let (role, source) = identify_process(
            Some(Path::new("/usr/bin/python3.11")),
            b"/usr/bin/python3\0/usr/bin/axis\0-ini\0dmc2.ini\0",
            Some(b"axis\n"),
        )
        .expect("valid catalog")
        .expect("catalogued AXIS process");

        assert_eq!(role.name(), "axis");
        assert_eq!(source, IdentitySource::CommandLine);
    }

    #[test]
    fn identifies_a_zombie_safe_role_from_linux_comm() {
        let (role, source) = identify_process(None, b"", Some(b"linuxcncsvr\n"))
            .expect("valid catalog")
            .expect("catalogued server process");

        assert_eq!(role.name(), "linuxcncsvr");
        assert_eq!(source, IdentitySource::ProcessComm);
    }
}
