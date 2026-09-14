use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use dmc2_linuxcnc_interface::{LINUXCNC_SOURCE_COMMIT, LINUXCNC_VERSION};

use crate::application::journal_error::{AtomicPublishStep, JournalError, JournalKind};
use crate::application::nml::{PollCodes, TransportStatus};

use super::native::ERROR_OBJECT_CAPACITY;
use super::record::ErrorMessageRecord;

pub(super) const JOURNAL_SCHEMA_VERSION: u32 = 4;
const HEADER_MARKER: &str = "DMC2_ERROR_JOURNAL";
const EVENT_MARKER: &str = "DMC2_ERROR_EVENT";
const FNV64_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV64_PRIME: u64 = 0x100000001b3;
const NATIVE_BYTE_ORDER: &str = if cfg!(target_endian = "little") {
    "little"
} else {
    "big"
};

pub(in crate::application) struct ErrorJournal {
    path: PathBuf,
    file: File,
    sequence: u64,
}

impl ErrorJournal {
    pub(in crate::application) fn create(
        path: &Path,
        codes: PollCodes,
    ) -> Result<Self, JournalError> {
        let parent = path
            .parent()
            .filter(|candidate| !candidate.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let metadata = parent
            .metadata()
            .map_err(|source| JournalError::ParentUnavailable {
                kind: JournalKind::LinuxCncErrorChannel,
                path: parent.to_owned(),
                source,
            })?;
        if !metadata.is_dir() {
            return Err(JournalError::ParentNotDirectory {
                kind: JournalKind::LinuxCncErrorChannel,
                path: parent.to_owned(),
            });
        }
        match path.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(JournalError::TargetNotRegular {
                    kind: JournalKind::LinuxCncErrorChannel,
                    path: path.to_owned(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(JournalError::TargetInspectionFailed {
                    kind: JournalKind::LinuxCncErrorChannel,
                    path: path.to_owned(),
                    source,
                });
            }
        }
        let file_name = path
            .file_name()
            .ok_or_else(|| JournalError::FilenameMissing {
                kind: JournalKind::LinuxCncErrorChannel,
                path: path.to_owned(),
            })?;
        let mut file_and_temporary = None;
        for attempt in 0..100_u32 {
            let mut temporary_name = OsString::from(".");
            temporary_name.push(file_name);
            temporary_name.push(format!(".{}.{}.tmp", std::process::id(), attempt));
            let temporary_path = parent.join(temporary_name);
            match OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary_path)
            {
                Ok(file) => {
                    file_and_temporary = Some((file, temporary_path));
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(source) => {
                    return Err(JournalError::TemporaryCreateFailed {
                        kind: JournalKind::LinuxCncErrorChannel,
                        path: temporary_path,
                        source,
                    });
                }
            }
        }
        let (mut file, temporary_path) =
            file_and_temporary.ok_or_else(|| JournalError::TemporaryNamesExhausted {
                kind: JournalKind::LinuxCncErrorChannel,
                path: path.to_owned(),
            })?;
        let header = header_line(codes);
        let preparation: Result<(), (AtomicPublishStep, std::io::Error)> = (|| {
            file.write_all(header.as_bytes())
                .map_err(|source| (AtomicPublishStep::WriteHeader, source))?;
            file.sync_all()
                .map_err(|source| (AtomicPublishStep::SyncTemporary, source))?;
            fs::rename(&temporary_path, path)
                .map_err(|source| (AtomicPublishStep::Rename, source))?;
            let parent_file =
                File::open(parent).map_err(|source| (AtomicPublishStep::OpenParent, source))?;
            parent_file
                .sync_all()
                .map_err(|source| (AtomicPublishStep::SyncParent, source))
        })();
        if let Err((step, source)) = preparation {
            let cleanup = match fs::remove_file(&temporary_path) {
                Ok(()) => None,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => Some((temporary_path.clone(), error)),
            };
            return Err(JournalError::AtomicPublishFailed {
                kind: JournalKind::LinuxCncErrorChannel,
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

    pub(in crate::application) fn append(
        &mut self,
        transport: TransportStatus,
        record: &ErrorMessageRecord,
    ) -> Result<u64, JournalError> {
        let sequence =
            self.sequence
                .checked_add(1)
                .ok_or_else(|| JournalError::SequenceExhausted {
                    kind: JournalKind::LinuxCncErrorChannel,
                    path: self.path.clone(),
                })?;
        let encoded = encode_event(sequence, transport, record);
        self.file
            .write_all(&encoded)
            .map_err(|source| JournalError::AppendFailed {
                kind: JournalKind::LinuxCncErrorChannel,
                path: self.path.clone(),
                source,
            })?;
        self.file
            .sync_data()
            .map_err(|source| JournalError::SyncFailed {
                kind: JournalKind::LinuxCncErrorChannel,
                path: self.path.clone(),
                source,
            })?;
        self.sequence = sequence;
        Ok(sequence)
    }
}

fn header_line(codes: PollCodes) -> String {
    let prefix = format!(
        "{HEADER_MARKER}\t{JOURNAL_SCHEMA_VERSION}\t{LINUXCNC_VERSION}\t{LINUXCNC_SOURCE_COMMIT}\t{ERROR_OBJECT_CAPACITY}\t{NATIVE_BYTE_ORDER}\t{}\t{}",
        codes.no_error, codes.cms_read_ok,
    );
    let checksum = fnv64(prefix.as_bytes());
    format!("{prefix}\t{checksum:016x}\n")
}

fn encode_event(sequence: u64, transport: TransportStatus, record: &ErrorMessageRecord) -> Vec<u8> {
    let source = record.cause().contract();
    let serial = record
        .serial_number
        .map_or_else(|| "-".to_owned(), |value| value.to_string());
    let operator_id = record
        .operator_id
        .map_or_else(|| "-".to_owned(), |value| value.to_string());
    let prefix = [
        EVENT_MARKER.to_owned(),
        JOURNAL_SCHEMA_VERSION.to_string(),
        sequence.to_string(),
        record.message_type.to_string(),
        record.class_name().to_owned(),
        record.severity.journal_name().to_owned(),
        u8::from(record.known()).to_string(),
        record.recovery_class().wire_code().to_string(),
        record.object_size.to_string(),
        record.declared_size.to_string(),
        serial,
        operator_id,
        encode_hex(&record.payload),
        encode_hex(&record.text),
        encode_hex(&record.padding),
        encode_hex(&record.object[..record.object_size]),
        transport.nml_error.to_string(),
        transport.cms_status.to_string(),
        source.identity.to_owned(),
        encode_hex(source.cause.as_bytes()),
        encode_hex(source.action.as_bytes()),
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
