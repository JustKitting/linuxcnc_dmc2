//! Parser for the exact DMC2 script-contract v1 grammar documented in
//! `docs/script-contract.md`.

use std::fmt;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};

use crate::catalog::Prerequisite;

const HEADER_MAGIC: &str = "(DMC2 SCRIPT 1)";
const HEADER_END: &str = "(DMC2 END)";
const HEADER_PREFIX: &str = "(DMC2 ";
const MAX_HEADER_LINES: usize = 128;
const MAX_HEADER_LINE_BYTES: usize = 4096;
const FNV1A64_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV1A64_PRIME: u64 = 0x100000001b3;

pub const INSPECTION_FORMAT: &str = "DMC2_SCRIPT_CONTRACT_V1";

const CONSERVATIVE_PREREQUISITES: &[Prerequisite] = &[
    Prerequisite::RunningSession,
    Prerequisite::EstopClear,
    Prerequisite::MachineOn,
    Prerequisite::InterpreterIdle,
    Prerequisite::AllHomed,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractSource {
    Header,
    ConservativeDefault,
}

impl ContractSource {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::ConservativeDefault => "conservative-default",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ScriptEffect {
    AxisMotion,
    Spindle,
    ProbePower,
    Coolant,
    ToolChange,
    DigitalOutput,
    CoordinateState,
    ExternalCommand,
    UnclassifiedMachineCode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentRevision {
    bytes: u64,
    fnv1a64: u64,
}

impl ContentRevision {
    pub const fn bytes(self) -> u64 {
        self.bytes
    }

    pub const fn fnv1a64(self) -> u64 {
        self.fnv1a64
    }
}

impl ScriptEffect {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "axis-motion" => Some(Self::AxisMotion),
            "spindle" => Some(Self::Spindle),
            "probe-power" => Some(Self::ProbePower),
            "coolant" => Some(Self::Coolant),
            "tool-change" => Some(Self::ToolChange),
            "digital-output" => Some(Self::DigitalOutput),
            "coordinate-state" => Some(Self::CoordinateState),
            "external-command" => Some(Self::ExternalCommand),
            "unclassified-machine-code" => Some(Self::UnclassifiedMachineCode),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::AxisMotion => "axis-motion",
            Self::Spindle => "spindle",
            Self::ProbePower => "probe-power",
            Self::Coolant => "coolant",
            Self::ToolChange => "tool-change",
            Self::DigitalOutput => "digital-output",
            Self::CoordinateState => "coordinate-state",
            Self::ExternalCommand => "external-command",
            Self::UnclassifiedMachineCode => "unclassified-machine-code",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptContract {
    path: PathBuf,
    source: ContractSource,
    effects: Vec<ScriptEffect>,
    prerequisites: Vec<Prerequisite>,
    recovery: RecoveryClass,
    revision: ContentRevision,
}

impl ScriptContract {
    pub fn open(requested: &Path) -> Result<Self, ScriptError> {
        let path = fs::canonicalize(requested).map_err(|source| ScriptError::Canonicalize {
            requested: requested.to_path_buf(),
            source,
        })?;
        let metadata = fs::metadata(&path).map_err(|source| ScriptError::Metadata {
            path: path.clone(),
            source,
        })?;
        if !metadata.is_file() {
            return Err(ScriptError::NotRegularFile { path });
        }
        let mut file = File::open(&path).map_err(|source| ScriptError::Open {
            path: path.clone(),
            source,
        })?;
        let revision = content_revision(&mut file).map_err(|source| ScriptError::RevisionRead {
            path: path.clone(),
            source,
        })?;
        file.seek(SeekFrom::Start(0))
            .map_err(|source| ScriptError::RevisionRewind {
                path: path.clone(),
                source,
            })?;
        parse_header(&path, BufReader::new(file), revision)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn source(&self) -> ContractSource {
        self.source
    }

    pub fn effects(&self) -> &[ScriptEffect] {
        &self.effects
    }

    pub fn prerequisites(&self) -> &[Prerequisite] {
        &self.prerequisites
    }

    pub const fn recovery(&self) -> RecoveryClass {
        self.recovery
    }

    pub const fn revision(&self) -> ContentRevision {
        self.revision
    }
}

fn content_revision(mut reader: impl Read) -> Result<ContentRevision, std::io::Error> {
    let mut bytes = 0_u64;
    let mut fnv1a64 = FNV1A64_OFFSET_BASIS;
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        for byte in &buffer[..count] {
            fnv1a64 ^= u64::from(*byte);
            fnv1a64 = fnv1a64.wrapping_mul(FNV1A64_PRIME);
        }
    }
    Ok(ContentRevision { bytes, fnv1a64 })
}

fn parse_header(
    path: &Path,
    mut reader: impl BufRead,
    revision: ContentRevision,
) -> Result<ScriptContract, ScriptError> {
    let mut header_started = false;
    let mut effects = None;
    let mut prerequisites = None;
    let mut recovery = None;
    let mut line = String::new();

    for line_number in 1..=MAX_HEADER_LINES {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|source| ScriptError::Read {
                path: path.to_path_buf(),
                line: line_number,
                source,
            })?;
        if bytes == 0 {
            break;
        }
        let value = line.trim();
        if bytes > MAX_HEADER_LINE_BYTES && (header_started || value.starts_with("(DMC2 SCRIPT")) {
            return Err(ScriptError::HeaderLineTooLong {
                path: path.to_path_buf(),
                line: line_number,
                bytes,
            });
        }
        if !header_started {
            if value == HEADER_MAGIC {
                header_started = true;
                continue;
            }
            if value.starts_with("(DMC2 SCRIPT") {
                return Err(ScriptError::HeaderMagic {
                    path: path.to_path_buf(),
                    line: line_number,
                    observed: value.to_owned(),
                });
            }
            if leading_non_code(value) {
                continue;
            }
            return Ok(conservative_contract(path, revision));
        }

        if value == HEADER_END {
            return complete_header(path, effects, prerequisites, recovery, revision);
        }
        if value.is_empty() {
            continue;
        }
        let Some(inner) = value
            .strip_prefix(HEADER_PREFIX)
            .and_then(|item| item.strip_suffix(')'))
        else {
            return Err(ScriptError::HeaderUnexpectedLine {
                path: path.to_path_buf(),
                line: line_number,
                observed: value.to_owned(),
            });
        };
        let Some((field, raw)) = inner.split_once(' ') else {
            return Err(ScriptError::HeaderUnexpectedLine {
                path: path.to_path_buf(),
                line: line_number,
                observed: value.to_owned(),
            });
        };
        match field {
            "EFFECTS" => set_once(
                path,
                line_number,
                "EFFECTS",
                &mut effects,
                parse_effects(path, line_number, raw)?,
            )?,
            "REQUIRES" => set_once(
                path,
                line_number,
                "REQUIRES",
                &mut prerequisites,
                parse_prerequisites(path, line_number, raw)?,
            )?,
            "RECOVERY" => set_once(
                path,
                line_number,
                "RECOVERY",
                &mut recovery,
                RecoveryClass::from_slug(raw).ok_or_else(|| ScriptError::UnknownRecovery {
                    path: path.to_path_buf(),
                    line: line_number,
                    value: raw.to_owned(),
                })?,
            )?,
            _ => {
                return Err(ScriptError::UnknownField {
                    path: path.to_path_buf(),
                    line: line_number,
                    field: field.to_owned(),
                })
            }
        }
    }

    if header_started {
        Err(ScriptError::UnterminatedHeader {
            path: path.to_path_buf(),
        })
    } else {
        Ok(conservative_contract(path, revision))
    }
}

fn leading_non_code(value: &str) -> bool {
    value.is_empty()
        || value == "%"
        || value.starts_with(';')
        || (value.starts_with('(') && value.ends_with(')'))
}

fn conservative_contract(path: &Path, revision: ContentRevision) -> ScriptContract {
    ScriptContract {
        path: path.to_path_buf(),
        source: ContractSource::ConservativeDefault,
        effects: vec![ScriptEffect::UnclassifiedMachineCode],
        prerequisites: CONSERVATIVE_PREREQUISITES.to_vec(),
        recovery: RecoveryClass::AbortTask,
        revision,
    }
}

fn complete_header(
    path: &Path,
    effects: Option<Vec<ScriptEffect>>,
    prerequisites: Option<Vec<Prerequisite>>,
    recovery: Option<RecoveryClass>,
    revision: ContentRevision,
) -> Result<ScriptContract, ScriptError> {
    Ok(ScriptContract {
        path: path.to_path_buf(),
        source: ContractSource::Header,
        effects: effects.ok_or_else(|| missing_field(path, "EFFECTS"))?,
        prerequisites: prerequisites.ok_or_else(|| missing_field(path, "REQUIRES"))?,
        recovery: recovery.ok_or_else(|| missing_field(path, "RECOVERY"))?,
        revision,
    })
}

fn missing_field(path: &Path, field: &'static str) -> ScriptError {
    ScriptError::MissingField {
        path: path.to_path_buf(),
        field,
    }
}

fn set_once<T>(
    path: &Path,
    line: usize,
    field: &'static str,
    destination: &mut Option<T>,
    value: T,
) -> Result<(), ScriptError> {
    if destination.is_some() {
        return Err(ScriptError::DuplicateField {
            path: path.to_path_buf(),
            line,
            field,
        });
    }
    *destination = Some(value);
    Ok(())
}

fn parse_effects(path: &Path, line: usize, raw: &str) -> Result<Vec<ScriptEffect>, ScriptError> {
    parse_list(path, line, "EFFECTS", raw, |value| {
        ScriptEffect::parse(value).ok_or_else(|| ScriptError::UnknownEffect {
            path: path.to_path_buf(),
            line,
            value: value.to_owned(),
        })
    })
}

fn parse_prerequisites(
    path: &Path,
    line: usize,
    raw: &str,
) -> Result<Vec<Prerequisite>, ScriptError> {
    parse_list(path, line, "REQUIRES", raw, |value| {
        let prerequisite =
            Prerequisite::parse(value).ok_or_else(|| ScriptError::UnknownPrerequisite {
                path: path.to_path_buf(),
                line,
                value: value.to_owned(),
            })?;
        match prerequisite {
            Prerequisite::RunningSession
            | Prerequisite::EstopClear
            | Prerequisite::MachineOn
            | Prerequisite::InterpreterIdle
            | Prerequisite::AllHomed => Ok(prerequisite),
            Prerequisite::DesktopSession | Prerequisite::PhysicalEstopReleased => {
                Err(ScriptError::UnsupportedPrerequisite {
                    path: path.to_path_buf(),
                    line,
                    value: value.to_owned(),
                })
            }
        }
    })
}

fn parse_list<T: Copy + Eq>(
    path: &Path,
    line: usize,
    field: &'static str,
    raw: &str,
    mut parse: impl FnMut(&str) -> Result<T, ScriptError>,
) -> Result<Vec<T>, ScriptError> {
    if raw.is_empty() {
        return Err(ScriptError::EmptyField {
            path: path.to_path_buf(),
            line,
            field,
        });
    }
    let mut parsed = Vec::new();
    for item in raw.split(';') {
        if item.is_empty() {
            return Err(ScriptError::EmptyListItem {
                path: path.to_path_buf(),
                line,
                field,
            });
        }
        let value = parse(item)?;
        if parsed.contains(&value) {
            return Err(ScriptError::DuplicateListItem {
                path: path.to_path_buf(),
                line,
                field,
                value: item.to_owned(),
            });
        }
        parsed.push(value);
    }
    Ok(parsed)
}

#[derive(Debug)]
pub enum ScriptError {
    Canonicalize {
        requested: PathBuf,
        source: std::io::Error,
    },
    Metadata {
        path: PathBuf,
        source: std::io::Error,
    },
    NotRegularFile {
        path: PathBuf,
    },
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    RevisionRead {
        path: PathBuf,
        source: std::io::Error,
    },
    RevisionRewind {
        path: PathBuf,
        source: std::io::Error,
    },
    Read {
        path: PathBuf,
        line: usize,
        source: std::io::Error,
    },
    HeaderLineTooLong {
        path: PathBuf,
        line: usize,
        bytes: usize,
    },
    HeaderMagic {
        path: PathBuf,
        line: usize,
        observed: String,
    },
    HeaderUnexpectedLine {
        path: PathBuf,
        line: usize,
        observed: String,
    },
    UnknownField {
        path: PathBuf,
        line: usize,
        field: String,
    },
    DuplicateField {
        path: PathBuf,
        line: usize,
        field: &'static str,
    },
    MissingField {
        path: PathBuf,
        field: &'static str,
    },
    EmptyField {
        path: PathBuf,
        line: usize,
        field: &'static str,
    },
    EmptyListItem {
        path: PathBuf,
        line: usize,
        field: &'static str,
    },
    DuplicateListItem {
        path: PathBuf,
        line: usize,
        field: &'static str,
        value: String,
    },
    UnknownEffect {
        path: PathBuf,
        line: usize,
        value: String,
    },
    UnknownPrerequisite {
        path: PathBuf,
        line: usize,
        value: String,
    },
    UnsupportedPrerequisite {
        path: PathBuf,
        line: usize,
        value: String,
    },
    UnknownRecovery {
        path: PathBuf,
        line: usize,
        value: String,
    },
    UnterminatedHeader {
        path: PathBuf,
    },
}

impl fmt::Display for ScriptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Canonicalize { requested, source } => write!(formatter, "SCRIPT_PATH_UNAVAILABLE: requested={} cause={source}; action: choose an existing readable machine-code file", requested.display()),
            Self::Metadata { path, source } => write!(formatter, "SCRIPT_METADATA_UNAVAILABLE: path={} cause={source}; action: restore access or choose another machine-code file", path.display()),
            Self::NotRegularFile { path } => write!(formatter, "SCRIPT_PATH_NOT_REGULAR_FILE: path={}; action: choose a regular machine-code file", path.display()),
            Self::Open { path, source } => write!(formatter, "SCRIPT_OPEN_FAILED: path={} cause={source}; action: restore read access or choose another machine-code file", path.display()),
            Self::RevisionRead { path, source } => write!(formatter, "SCRIPT_REVISION_READ_FAILED: path={} cause={source}; action: restore stable read access or choose another machine-code file", path.display()),
            Self::RevisionRewind { path, source } => write!(formatter, "SCRIPT_REVISION_REWIND_FAILED: path={} cause={source}; action: use a seekable regular machine-code file", path.display()),
            Self::Read { path, line, source } => write!(formatter, "SCRIPT_HEADER_READ_FAILED: path={} line={line} cause={source}; action: correct the text encoding/read error or choose another file", path.display()),
            Self::HeaderLineTooLong { path, line, bytes } => write!(formatter, "SCRIPT_HEADER_LINE_TOO_LONG: path={} line={line} bytes={bytes} maximum={MAX_HEADER_LINE_BYTES}; action: shorten the DMC2 header line", path.display()),
            Self::HeaderMagic { path, line, observed } => write!(formatter, "SCRIPT_HEADER_MAGIC_INVALID: path={} line={line} observed={observed:?} expected={HEADER_MAGIC:?}; action: correct the DMC2 header or remove it to use the conservative contract", path.display()),
            Self::HeaderUnexpectedLine { path, line, observed } => write!(formatter, "SCRIPT_HEADER_LINE_INVALID: path={} line={line} observed={observed:?}; action: use only DMC2 EFFECTS, REQUIRES, and RECOVERY fields before {HEADER_END}", path.display()),
            Self::UnknownField { path, line, field } => write!(formatter, "SCRIPT_HEADER_FIELD_UNKNOWN: path={} line={line} field={field:?}; action: use an exact supported DMC2 header field", path.display()),
            Self::DuplicateField { path, line, field } => write!(formatter, "SCRIPT_HEADER_FIELD_DUPLICATE: path={} line={line} field={field}; action: declare each DMC2 header field exactly once", path.display()),
            Self::MissingField { path, field } => write!(formatter, "SCRIPT_HEADER_FIELD_MISSING: path={} field={field}; action: declare EFFECTS, REQUIRES, and RECOVERY exactly once", path.display()),
            Self::EmptyField { path, line, field } => write!(formatter, "SCRIPT_HEADER_FIELD_EMPTY: path={} line={line} field={field}; action: provide at least one typed value", path.display()),
            Self::EmptyListItem { path, line, field } => write!(formatter, "SCRIPT_HEADER_LIST_ITEM_EMPTY: path={} line={line} field={field}; action: remove leading, trailing, or repeated semicolons", path.display()),
            Self::DuplicateListItem { path, line, field, value } => write!(formatter, "SCRIPT_HEADER_LIST_ITEM_DUPLICATE: path={} line={line} field={field} value={value:?}; action: list each typed value once", path.display()),
            Self::UnknownEffect { path, line, value } => write!(formatter, "SCRIPT_EFFECT_UNKNOWN: path={} line={line} value={value:?}; action: use a documented DMC2 script effect", path.display()),
            Self::UnknownPrerequisite { path, line, value } => write!(formatter, "SCRIPT_PREREQUISITE_UNKNOWN: path={} line={line} value={value:?}; action: use a documented DMC2 script prerequisite", path.display()),
            Self::UnsupportedPrerequisite { path, line, value } => write!(formatter, "SCRIPT_PREREQUISITE_UNSUPPORTED: path={} line={line} value={value:?}; action: use only running-session, estop-clear, machine-on, interpreter-idle, or all-homed", path.display()),
            Self::UnknownRecovery { path, line, value } => write!(formatter, "SCRIPT_RECOVERY_UNKNOWN: path={} line={line} value={value:?}; action: use a recovery slug from the DMC2 recovery catalog", path.display()),
            Self::UnterminatedHeader { path } => write!(formatter, "SCRIPT_HEADER_UNTERMINATED: path={} maximum_header_lines={MAX_HEADER_LINES}; action: add the exact {HEADER_END} line", path.display()),
        }
    }
}

impl RecoveryClassified for ScriptError {
    fn recovery_class(&self) -> RecoveryClass {
        RecoveryClass::RecheckSource
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(content: &[u8]) -> Result<ScriptContract, ScriptError> {
        let revision = content_revision(Cursor::new(content)).expect("memory read cannot fail");
        parse_header(Path::new("job.ngc"), Cursor::new(content), revision)
    }

    #[test]
    fn exact_header_produces_typed_contract() {
        let content = b"%\n(DMC2 SCRIPT 1)\n(DMC2 EFFECTS axis-motion;probe-power)\n(DMC2 REQUIRES running-session;estop-clear;machine-on;interpreter-idle)\n(DMC2 RECOVERY abort-task)\n(DMC2 END)\nG21\n";
        let contract = parse(content).expect("exact header should parse");
        assert_eq!(contract.source(), ContractSource::Header);
        assert_eq!(
            contract.effects(),
            &[ScriptEffect::AxisMotion, ScriptEffect::ProbePower]
        );
        assert_eq!(contract.recovery(), RecoveryClass::AbortTask);
    }

    #[test]
    fn headerless_code_receives_conservative_contract() {
        let contract = parse(b"G0 X1\n").expect("headerless G-code should receive defaults");
        assert_eq!(contract.source(), ContractSource::ConservativeDefault);
        assert_eq!(contract.effects(), &[ScriptEffect::UnclassifiedMachineCode]);
        assert!(contract.prerequisites().contains(&Prerequisite::AllHomed));
    }

    #[test]
    fn descriptive_dmc2_comment_is_not_mistaken_for_a_header() {
        let content = b"(DMC2 X PROBE-OFFSET: SELECTED CONTACT INPUT)\nG38.2 X1\n";
        let revision = content_revision(Cursor::new(content)).expect("memory read cannot fail");
        let contract = parse_header(Path::new("probe.ngc"), Cursor::new(content), revision)
            .expect("a legacy descriptive comment is ordinary G-code commentary");
        assert_eq!(contract.source(), ContractSource::ConservativeDefault);
    }
}
