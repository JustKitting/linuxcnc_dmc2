use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};

const CATALOG: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../config/processes.tsv"
));
const MAGIC: &str = "DMC2_PROCESS_CATALOG\t2";
const HEADER: &str = "role\tprogram\tlaunch_site\townership\tcriticality\tbacktrace\targument_placement\tcore_dump_policy\towner_comm";

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
    pub const fn core_dump_policy(self) -> CoreDumpPolicy {
        self.definition.core_dump_policy
    }
    pub const fn owner_comm(self) -> &'static str {
        self.definition.owner_comm
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
    core_dump_policy: CoreDumpPolicy,
    owner_comm: &'static str,
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
}

impl Criticality {
    pub const fn name(self) -> &'static str {
        match self {
            Self::ControllerCritical => "controller-critical",
            Self::OperatorInterface => "operator-interface",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreDumpPolicy {
    Inherit,
    EnableToHardLimit,
}

impl CoreDumpPolicy {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::EnableToHardLimit => "enable-to-hard-limit",
        }
    }
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
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    let comm = comm.map(trim_ascii_whitespace);
    for definition in definitions()? {
        let expected = Path::new(definition.program);
        let executable_matches = executable.is_some_and(|observed| {
            if expected.is_absolute() {
                observed.as_os_str().as_bytes() == expected.as_os_str().as_bytes()
            } else {
                observed.file_name() == expected.file_name()
            }
        });
        if executable_matches {
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

pub fn identify_process_owner(comm: &[u8]) -> Result<Option<ProcessRole>, CatalogError> {
    let observed = trim_ascii_whitespace(comm);
    Ok(definitions()?
        .filter(|definition| definition.owner_comm != "none")
        .find(|definition| definition.owner_comm.as_bytes() == observed)
        .map(|definition| ProcessRole { definition }))
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
    let definitions = lines
        .enumerate()
        .filter(|(_, line)| !line.is_empty())
        .map(|(index, line)| parse_definition(index + 3, line))
        .collect::<Result<Vec<_>, _>>()?;
    validate_definitions(&definitions)?;
    Ok(definitions.into_iter())
}

fn validate_definitions(definitions: &[ProcessDefinition]) -> Result<(), CatalogError> {
    let mut roles = BTreeSet::new();
    let mut owners = BTreeSet::new();
    for definition in definitions {
        if !roles.insert(definition.role) {
            return Err(CatalogError::DuplicateRole(definition.role.to_owned()));
        }
        let owns_child = matches!(
            definition.ownership,
            Ownership::DirectChild | Ownership::SessionRoot
        );
        if owns_child != (definition.owner_comm != "none") {
            return Err(CatalogError::OwnerCommContract {
                role: definition.role.to_owned(),
                ownership: definition.ownership,
                owner_comm: definition.owner_comm.to_owned(),
            });
        }
        if definition.owner_comm != "none" && !owners.insert(definition.owner_comm) {
            return Err(CatalogError::DuplicateOwnerComm(
                definition.owner_comm.to_owned(),
            ));
        }
    }
    Ok(())
}

fn parse_definition(
    line_number: usize,
    line: &'static str,
) -> Result<ProcessDefinition, CatalogError> {
    let fields = line.split('\t').collect::<Vec<_>>();
    let [role, program, launch_site, ownership, criticality, backtrace, argument_placement, core_dump_policy, owner_comm] =
        fields.as_slice()
    else {
        return Err(CatalogError::FieldCount {
            line: line_number,
            observed: fields.len(),
        });
    };
    for (field, value) in [
        ("role", *role),
        ("program", *program),
        ("launch_site", *launch_site),
    ] {
        if value.is_empty() {
            return Err(CatalogError::EmptyField {
                line: line_number,
                field,
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
        core_dump_policy: parse_core_dump_policy(line_number, core_dump_policy)?,
        owner_comm: parse_owner_comm(line_number, owner_comm)?,
    })
}

fn value_error(line: usize, field: &'static str, value: &str) -> CatalogError {
    CatalogError::Value {
        line,
        field,
        value: value.to_owned(),
    }
}

fn parse_ownership(line: usize, value: &str) -> Result<Ownership, CatalogError> {
    match value {
        "direct-child" => Ok(Ownership::DirectChild),
        "session-root" => Ok(Ownership::SessionRoot),
        "self-daemonizing-descendant" => Ok(Ownership::SelfDaemonizingDescendant),
        "persistent-master" => Ok(Ownership::PersistentMaster),
        _ => Err(value_error(line, "ownership", value)),
    }
}

fn parse_criticality(line: usize, value: &str) -> Result<Criticality, CatalogError> {
    match value {
        "controller-critical" => Ok(Criticality::ControllerCritical),
        "operator-interface" => Ok(Criticality::OperatorInterface),
        _ => Err(value_error(line, "criticality", value)),
    }
}

fn parse_backtrace(line: usize, value: &str) -> Result<BacktraceKind, CatalogError> {
    match value {
        "none" => Ok(BacktraceKind::None),
        "linuxcnc-task" => Ok(BacktraceKind::LinuxCncTask),
        _ => Err(value_error(line, "backtrace", value)),
    }
}

fn parse_argument_placement(line: usize, value: &str) -> Result<ArgumentPlacement, CatalogError> {
    match value {
        "none" => Ok(ArgumentPlacement::None),
        "linuxcnc-appends-ini" => Ok(ArgumentPlacement::LinuxCncAppendsIni),
        "linuxcnc-prepends-ini" => Ok(ArgumentPlacement::LinuxCncPrependsIni),
        _ => Err(value_error(line, "argument_placement", value)),
    }
}

fn parse_core_dump_policy(line: usize, value: &str) -> Result<CoreDumpPolicy, CatalogError> {
    match value {
        "inherit" => Ok(CoreDumpPolicy::Inherit),
        "enable-to-hard-limit" => Ok(CoreDumpPolicy::EnableToHardLimit),
        _ => Err(value_error(line, "core_dump_policy", value)),
    }
}

fn parse_owner_comm(line: usize, value: &'static str) -> Result<&'static str, CatalogError> {
    if value.is_empty() || value.len() > 15 || value.as_bytes().contains(&0) {
        return Err(value_error(line, "owner_comm", value));
    }
    Ok(value)
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
    DuplicateRole(String),
    DuplicateOwnerComm(String),
    OwnerCommContract {
        role: String,
        ownership: Ownership,
        owner_comm: String,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Magic(observed) => write!(formatter, "process catalog version mismatch: observed {observed:?}; rebuild and relaunch from the desktop application"),
            Self::Header(observed) => write!(formatter, "process catalog header mismatch: observed {observed:?}; restore the reviewed catalog and rebuild"),
            Self::FieldCount { line, observed } => write!(formatter, "process catalog line {line} has {observed} fields; expected 9"),
            Self::EmptyField { line, field } => write!(formatter, "process catalog line {line} has an empty {field}"),
            Self::Value { line, field, value } => write!(formatter, "process catalog line {line} has unknown {field} value {value:?}"),
            Self::NonUtf8Role(value) => write!(formatter, "process role is not UTF-8: {value:?}"),
            Self::UnknownRole(value) => write!(formatter, "process role is not catalogued: {value:?}"),
            Self::DuplicateRole(value) => write!(formatter, "process role is duplicated: {value:?}"),
            Self::DuplicateOwnerComm(value) => write!(formatter, "process owner name is duplicated: {value:?}"),
            Self::OwnerCommContract { role, ownership, owner_comm } => write!(formatter, "process role {role:?} with ownership {} has invalid owner name {owner_comm:?}", ownership.name()),
        }
    }
}

impl std::error::Error for CatalogError {}

impl RecoveryClassified for CatalogError {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::Magic(_)
            | Self::Header(_)
            | Self::FieldCount { .. }
            | Self::EmptyField { .. }
            | Self::Value { .. }
            | Self::NonUtf8Role(_)
            | Self::UnknownRole(_)
            | Self::DuplicateRole(_)
            | Self::DuplicateOwnerComm(_)
            | Self::OwnerCommContract { .. } => RecoveryClass::RelaunchApplication,
        }
    }
}
