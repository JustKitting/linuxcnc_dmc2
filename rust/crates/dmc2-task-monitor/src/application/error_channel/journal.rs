use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use dmc2_linuxcnc_interface::{LINUXCNC_SOURCE_COMMIT, LINUXCNC_VERSION};

use crate::application::nml::TransportStatus;

use super::native::ERROR_OBJECT_CAPACITY;
use super::record::ErrorMessageRecord;

pub(super) const JOURNAL_SCHEMA_VERSION: u32 = 1;
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
    pub(in crate::application) fn create(path: &Path) -> Result<Self, String> {
        let parent = path
            .parent()
            .filter(|candidate| !candidate.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let metadata = parent.metadata().map_err(|error| {
            format!(
                "error-channel journal parent {} is unavailable: {error}",
                parent.display()
            )
        })?;
        if !metadata.is_dir() {
            return Err(format!(
                "error-channel journal parent is not a directory: {}",
                parent.display()
            ));
        }
        match path.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(format!(
                    "error-channel journal path is not a regular file: {}",
                    path.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "failed to inspect error-channel journal {}: {error}",
                    path.display()
                ));
            }
        }
        let file_name = path
            .file_name()
            .ok_or_else(|| format!("error-channel journal has no file name: {}", path.display()))?;
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
                Err(error) => {
                    return Err(format!(
                        "failed to create error-channel journal temporary file {}: {error}",
                        temporary_path.display()
                    ));
                }
            }
        }
        let (mut file, temporary_path) = file_and_temporary.ok_or_else(|| {
            format!(
                "failed to reserve an error-channel journal temporary file beside {}",
                path.display()
            )
        })?;
        let header = header_line();
        let preparation = (|| {
            file.write_all(header.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary_path, path)?;
            File::open(parent)?.sync_all()
        })();
        if let Err(error) = preparation {
            let cleanup = match fs::remove_file(&temporary_path) {
                Ok(()) => String::new(),
                Err(cleanup_error) if cleanup_error.kind() == std::io::ErrorKind::NotFound => {
                    String::new()
                }
                Err(cleanup_error) => format!(
                    "; additionally failed to remove temporary file {}: {cleanup_error}",
                    temporary_path.display()
                ),
            };
            return Err(format!(
                "failed to atomically publish error-channel journal {}: {error}{cleanup}",
                path.display()
            ));
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
    ) -> Result<u64, String> {
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| "error-channel journal sequence exhausted".to_owned())?;
        let encoded = encode_event(sequence, transport, record);
        self.file.write_all(&encoded).map_err(|error| {
            format!(
                "failed to append error-channel journal {}: {error}",
                self.path.display()
            )
        })?;
        self.file.sync_data().map_err(|error| {
            format!(
                "failed to synchronize error-channel journal {}: {error}",
                self.path.display()
            )
        })?;
        self.sequence = sequence;
        Ok(sequence)
    }
}

fn header_line() -> String {
    format!(
        "{HEADER_MARKER}\t{JOURNAL_SCHEMA_VERSION}\t{LINUXCNC_VERSION}\t{LINUXCNC_SOURCE_COMMIT}\t{ERROR_OBJECT_CAPACITY}\t{NATIVE_BYTE_ORDER}\n"
    )
}

fn encode_event(sequence: u64, transport: TransportStatus, record: &ErrorMessageRecord) -> Vec<u8> {
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
    ]
    .join("\t");
    let checksum = fnv64(prefix.as_bytes());
    format!("{prefix}\t{checksum:016x}\n").into_bytes()
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = Vec::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)]);
        output.push(HEX[usize::from(byte & 0x0f)]);
    }
    String::from_utf8(output).expect("hex encoding is ASCII")
}

fn fnv64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV64_OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV64_PRIME)
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::fd::{FromRawFd, IntoRawFd};
    use std::os::unix::fs::symlink;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicU64, Ordering};

    use dmc2_linuxcnc_interface::ERROR_MESSAGE_CONTRACTS;

    use super::*;
    use crate::application::error_channel::record::ErrorSeverity;

    static NEXT_PATH: AtomicU64 = AtomicU64::new(0);

    fn record(index: usize) -> ErrorMessageRecord {
        let contract = ERROR_MESSAGE_CONTRACTS[index];
        let mut object = [0; ERROR_OBJECT_CAPACITY];
        for (offset, byte) in object[..contract.message_size].iter_mut().enumerate() {
            *byte = offset as u8;
        }
        ErrorMessageRecord {
            message_type: contract.message_type as i32,
            contract: Some(contract),
            severity: if contract.class_name.ends_with("_ERROR") {
                ErrorSeverity::Error
            } else {
                ErrorSeverity::Info
            },
            object_size: contract.message_size,
            declared_size: contract.message_size as i64,
            serial_number: contract.serial_offset.map(|_| 17),
            operator_id: contract.id_offset.map(|_| -19),
            payload: vec![0, 1, 127, 128, 254, 255],
            text: vec![0, 1, 127],
            padding: if contract.class_name.starts_with("NML_") {
                vec![0; 4]
            } else {
                vec![0; 5]
            },
            object,
        }
    }

    fn temporary_path() -> PathBuf {
        let id = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "dmc2-error-journal-test-{}-{id}.tsv",
            std::process::id()
        ))
    }

    #[test]
    fn header_pins_schema_source_and_capacity_exactly() {
        assert_eq!(
            header_line(),
            concat!(
                "DMC2_ERROR_JOURNAL\t1\t2.9.10\t",
                "86cdca76fa2a36274c432caa21952b23c267989a\t280\tlittle\n"
            )
        );
    }

    #[test]
    fn every_possible_byte_has_one_canonical_hex_encoding() {
        let bytes = (0..=u8::MAX).collect::<Vec<_>>();
        let encoded = encode_hex(&bytes);
        assert_eq!(encoded.len(), 512);
        assert_eq!(&encoded[..8], "00010203");
        assert_eq!(&encoded[504..], "fcfdfeff");
        assert!(encoded.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn all_six_event_forms_have_exact_fields_lengths_and_checksum() {
        let transport = TransportStatus {
            nml_error: 0,
            cms_status: 1,
        };
        for index in 0..ERROR_MESSAGE_CONTRACTS.len() {
            let record = record(index);
            let encoded = encode_event(index as u64 + 1, transport, &record);
            assert_eq!(encoded.last(), Some(&b'\n'));
            let line = std::str::from_utf8(&encoded[..encoded.len() - 1]).unwrap();
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 18);
            assert_eq!(fields[0], EVENT_MARKER);
            assert_eq!(fields[1], "1");
            assert_eq!(fields[2], (index + 1).to_string());
            assert_eq!(fields[3], record.message_type.to_string());
            assert_eq!(fields[4], record.class_name());
            assert_eq!(fields[6], "1");
            assert_eq!(fields[7], record.object_size.to_string());
            assert_eq!(fields[11].len(), record.payload.len() * 2);
            assert_eq!(fields[12].len(), record.text.len() * 2);
            assert_eq!(fields[13].len(), record.padding.len() * 2);
            assert_eq!(fields[14].len(), record.object_size * 2);
            let checksum_prefix = fields[..17].join("\t");
            assert_eq!(
                fields[17],
                format!("{:016x}", fnv64(checksum_prefix.as_bytes()))
            );
        }
    }

    #[test]
    fn journal_atomically_replaces_then_durably_appends_monotonic_records() {
        let path = temporary_path();
        fs::write(&path, b"stale\n").unwrap();
        let stale_inode = path.metadata().unwrap().ino();
        let transport = TransportStatus {
            nml_error: 0,
            cms_status: 1,
        };
        {
            let mut journal = ErrorJournal::create(&path).unwrap();
            assert_eq!(journal.append(transport, &record(0)).unwrap(), 1);
            assert_eq!(journal.append(transport, &record(5)).unwrap(), 2);
        }
        let contents = fs::read_to_string(&path).unwrap();
        assert_ne!(path.metadata().unwrap().ino(), stale_inode);
        let lines = contents.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with(HEADER_MARKER));
        assert_eq!(lines[1].split('\t').nth(2), Some("1"));
        assert_eq!(lines[2].split('\t').nth(2), Some("2"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn symlink_and_non_regular_journal_targets_are_rejected() {
        let directory = temporary_path();
        fs::create_dir(&directory).unwrap();
        assert!(ErrorJournal::create(&directory).is_err());
        fs::remove_dir(&directory).unwrap();

        let target = temporary_path();
        let link = temporary_path();
        fs::write(&target, b"target").unwrap();
        symlink(&target, &link).unwrap();
        assert!(ErrorJournal::create(&link).is_err());
        fs::remove_file(link).unwrap();
        fs::remove_file(target).unwrap();
    }

    #[test]
    fn every_preparation_path_shape_and_temporary_name_exhaustion_is_rejected() {
        let missing_parent = temporary_path();
        let missing_child = missing_parent.join("errors.tsv");
        assert!(ErrorJournal::create(&missing_child).is_err());

        let regular_parent = temporary_path();
        fs::write(&regular_parent, b"not a directory").unwrap();
        assert!(ErrorJournal::create(&regular_parent.join("errors.tsv")).is_err());
        fs::remove_file(regular_parent).unwrap();

        assert!(ErrorJournal::create(Path::new("")).is_err());

        let directory = temporary_path();
        fs::create_dir(&directory).unwrap();
        let path = directory.join("errors.tsv");
        let mut collisions = Vec::new();
        for attempt in 0..100_u32 {
            let collision =
                directory.join(format!(".errors.tsv.{}.{attempt}.tmp", std::process::id()));
            fs::write(&collision, b"occupied").unwrap();
            collisions.push(collision);
        }
        assert!(ErrorJournal::create(&path).is_err());
        for collision in collisions {
            fs::remove_file(collision).unwrap();
        }
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn append_write_sync_and_sequence_failures_are_all_visible() {
        let path = temporary_path();
        fs::write(&path, b"read only handle").unwrap();
        let read_only = OpenOptions::new().read(true).open(&path).unwrap();
        let mut journal = ErrorJournal {
            path: path.clone(),
            file: read_only,
            sequence: 0,
        };
        assert!(journal
            .append(
                TransportStatus {
                    nml_error: 0,
                    cms_status: 1,
                },
                &record(0),
            )
            .unwrap_err()
            .contains("failed to append"));
        fs::remove_file(&path).unwrap();

        let (writer, _reader) = UnixStream::pair().unwrap();
        let socket_file = unsafe { File::from_raw_fd(writer.into_raw_fd()) };
        let mut journal = ErrorJournal {
            path: path.clone(),
            file: socket_file,
            sequence: 0,
        };
        assert!(journal
            .append(
                TransportStatus {
                    nml_error: 0,
                    cms_status: 1,
                },
                &record(0),
            )
            .unwrap_err()
            .contains("failed to synchronize"));

        let writable = File::create(&path).unwrap();
        let mut journal = ErrorJournal {
            path: path.clone(),
            file: writable,
            sequence: u64::MAX,
        };
        assert_eq!(
            journal
                .append(
                    TransportStatus {
                        nml_error: 0,
                        cms_status: 1,
                    },
                    &record(0),
                )
                .unwrap_err(),
            "error-channel journal sequence exhausted"
        );
        fs::remove_file(path).unwrap();
    }
}
