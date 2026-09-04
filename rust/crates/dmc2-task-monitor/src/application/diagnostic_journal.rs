//! Durable, checksummed stream of fully self-describing diagnostic transitions.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use dmc2_diagnostics::{RecoveryClass, RECOVERY_CONTRACTS};
use dmc2_linuxcnc_interface::{LINUXCNC_SOURCE_COMMIT, LINUXCNC_VERSION};

use crate::application::journal_error::{AtomicPublishStep, JournalError, JournalKind};
use crate::diagnostics::DiagnosticTransition;

pub(super) const JOURNAL_SCHEMA_VERSION: u32 = 3;
const HEADER_MARKER: &str = "DMC2_DIAGNOSTIC_JOURNAL";
const RECOVERY_MARKER: &str = "DMC2_RECOVERY_CLASS";
const EVENT_MARKER: &str = "DMC2_DIAGNOSTIC_EVENT";
const FNV64_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV64_PRIME: u64 = 0x100000001b3;

#[derive(Debug)]
pub(super) struct DiagnosticJournal {
    path: PathBuf,
    file: File,
    sequence: u64,
}

impl DiagnosticJournal {
    pub(super) fn create(path: &Path) -> Result<Self, JournalError> {
        let parent = path
            .parent()
            .filter(|candidate| !candidate.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let metadata = parent
            .metadata()
            .map_err(|source| JournalError::ParentUnavailable {
                kind: JournalKind::Diagnostic,
                path: parent.to_owned(),
                source,
            })?;
        if !metadata.is_dir() {
            return Err(JournalError::ParentNotDirectory {
                kind: JournalKind::Diagnostic,
                path: parent.to_owned(),
            });
        }
        match path.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(JournalError::TargetNotRegular {
                    kind: JournalKind::Diagnostic,
                    path: path.to_owned(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(JournalError::TargetInspectionFailed {
                    kind: JournalKind::Diagnostic,
                    path: path.to_owned(),
                    source,
                });
            }
        }
        let file_name = path
            .file_name()
            .ok_or_else(|| JournalError::FilenameMissing {
                kind: JournalKind::Diagnostic,
                path: path.to_owned(),
            })?;
        let mut reserved = None;
        for attempt in 0..100_u32 {
            let mut name = OsString::from(".");
            name.push(file_name);
            name.push(format!(".{}.{}.tmp", std::process::id(), attempt));
            let temporary = parent.join(name);
            match OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
            {
                Ok(file) => {
                    reserved = Some((file, temporary));
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(source) => {
                    return Err(JournalError::TemporaryCreateFailed {
                        kind: JournalKind::Diagnostic,
                        path: temporary,
                        source,
                    });
                }
            }
        }
        let (mut file, temporary) =
            reserved.ok_or_else(|| JournalError::TemporaryNamesExhausted {
                kind: JournalKind::Diagnostic,
                path: path.to_owned(),
            })?;
        let preparation: Result<(), (AtomicPublishStep, std::io::Error)> = (|| {
            file.write_all(header_block().as_bytes())
                .map_err(|source| (AtomicPublishStep::WriteHeader, source))?;
            file.sync_all()
                .map_err(|source| (AtomicPublishStep::SyncTemporary, source))?;
            fs::rename(&temporary, path).map_err(|source| (AtomicPublishStep::Rename, source))?;
            let parent_file =
                File::open(parent).map_err(|source| (AtomicPublishStep::OpenParent, source))?;
            parent_file
                .sync_all()
                .map_err(|source| (AtomicPublishStep::SyncParent, source))
        })();
        if let Err((step, source)) = preparation {
            let cleanup = match fs::remove_file(&temporary) {
                Ok(()) => None,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => Some((temporary.clone(), error)),
            };
            return Err(JournalError::AtomicPublishFailed {
                kind: JournalKind::Diagnostic,
                path: path.to_owned(),
                step,
                source,
                cleanup,
            });
        }
        Ok(Self {
            path: path.to_owned(),
            file,
            sequence: 0,
        })
    }

    pub(super) fn append(
        &mut self,
        transition: &DiagnosticTransition,
    ) -> Result<u64, JournalError> {
        let sequence =
            self.sequence
                .checked_add(1)
                .ok_or_else(|| JournalError::SequenceExhausted {
                    kind: JournalKind::Diagnostic,
                    path: self.path.clone(),
                })?;
        let encoded = encode_event(sequence, transition);
        self.file
            .write_all(&encoded)
            .map_err(|source| JournalError::AppendFailed {
                kind: JournalKind::Diagnostic,
                path: self.path.clone(),
                source,
            })?;
        self.file
            .sync_data()
            .map_err(|source| JournalError::SyncFailed {
                kind: JournalKind::Diagnostic,
                path: self.path.clone(),
                source,
            })?;
        self.sequence = sequence;
        Ok(sequence)
    }
}

fn header_block() -> String {
    let mut block = format!(
        "{HEADER_MARKER}\t{JOURNAL_SCHEMA_VERSION}\t{LINUXCNC_VERSION}\t{LINUXCNC_SOURCE_COMMIT}\t{}\n",
        RecoveryClass::COUNT,
    );
    for contract in RECOVERY_CONTRACTS {
        block.push_str(&encode_recovery_class(contract.recovery_class()));
    }
    block
}

fn encode_recovery_class(recovery: RecoveryClass) -> String {
    let operations = recovery
        .ui_operations()
        .iter()
        .map(|operation| operation.id())
        .collect::<Vec<_>>()
        .join(";");
    let transition = recovery.transition();
    let prefix = [
        RECOVERY_MARKER.to_owned(),
        JOURNAL_SCHEMA_VERSION.to_string(),
        recovery.wire_code().to_string(),
        encode_hex(recovery.name().as_bytes()),
        encode_hex(recovery.hal_slug().as_bytes()),
        transition.wire_code().to_string(),
        encode_hex(transition.name().as_bytes()),
        encode_hex(transition.description().as_bytes()),
        encode_hex(operations.as_bytes()),
    ]
    .join("\t");
    let checksum = fnv64(prefix.as_bytes());
    format!("{prefix}\t{checksum:016x}\n")
}

fn encode_event(sequence: u64, transition: &DiagnosticTransition) -> Vec<u8> {
    let issue = &transition.issue;
    let prefix = [
        EVENT_MARKER.to_owned(),
        JOURNAL_SCHEMA_VERSION.to_string(),
        sequence.to_string(),
        transition.action.name().to_owned(),
        transition.action.wire_code().to_string(),
        issue.severity().as_str().to_owned(),
        format!("{:016x}", issue.category().mask()),
        issue.recovery_class().wire_code().to_string(),
        issue.domain_id().to_string(),
        issue.value().to_string(),
        encode_hex(issue.source().as_bytes()),
        encode_hex(issue.domain().as_bytes()),
        encode_hex(transition.identity().as_bytes()),
        u8::from(issue.name().is_some()).to_string(),
        encode_hex(issue.detail().as_bytes()),
        encode_hex(issue.operator_action().as_bytes()),
        encode_hex(issue.evidence().as_bytes()),
    ]
    .join("\t");
    let checksum = fnv64(prefix.as_bytes());
    format!("{prefix}\t{checksum:016x}\n").into_bytes()
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn fnv64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV64_OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV64_PRIME)
    })
}
