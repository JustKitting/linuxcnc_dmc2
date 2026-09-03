use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::event::Event;

pub struct Journal {
    file: File,
    path: PathBuf,
    recovered_partial_record: bool,
}

pub struct FailureTracker {
    first: Option<JournalError>,
    last: Option<JournalErrorEvidence>,
    failures: u64,
    recoveries: u64,
    failure_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JournalErrorEvidence {
    operation: &'static str,
    path: PathBuf,
    kind: io::ErrorKind,
    raw_os_error: Option<i32>,
    detail: String,
}

impl Journal {
    pub fn open(path: &Path) -> Result<Self, JournalError> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        if let Some(parent) = parent {
            fs::create_dir_all(parent)
                .map_err(|source| JournalError::io("create journal directory", parent, source))?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .mode(0o600)
            .open(path)
            .map_err(|source| JournalError::io("open journal", path, source))?;

        if let Some(parent) = parent {
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|source| {
                    JournalError::io("synchronize journal directory", parent, source)
                })?;
        }

        let recovered_partial_record = with_exclusive_lock(&mut file, path, |file| {
            let recovered = recover_partial_record(file, path)?;
            if let Some(recovered) = recovered {
                write_recovery_record(file, path, recovered)?;
                synchronize(file, path, "synchronize recovered journal record")?;
                Ok(true)
            } else {
                Ok(false)
            }
        })?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
            recovered_partial_record,
        })
    }

    pub fn recovered_partial_record(&self) -> bool {
        self.recovered_partial_record
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&mut self, event: &Event) -> Result<(), JournalError> {
        with_exclusive_lock(&mut self.file, &self.path, |file| {
            if let Some(recovered) = recover_partial_record(file, &self.path)? {
                write_recovery_record(file, &self.path, recovered)?;
                synchronize(file, &self.path, "synchronize recovered journal record")?;
            }
            write_event_record(file, &self.path, event)?;
            synchronize(file, &self.path, "synchronize journal record")
        })
    }
}

impl FailureTracker {
    pub fn new() -> Self {
        Self {
            first: None,
            last: None,
            failures: 0,
            recoveries: 0,
            failure_active: false,
        }
    }

    pub fn record_failure(&mut self, error: JournalError) {
        self.failures = self.failures.saturating_add(1);
        self.failure_active = true;
        self.last = Some(JournalErrorEvidence::capture(&error));
        if self.first.is_none() {
            self.first = Some(error);
        }
    }

    pub fn record_success(&mut self) {
        if self.failure_active {
            self.recoveries = self.recoveries.saturating_add(1);
            self.failure_active = false;
        }
    }

    pub fn take_first(&mut self) -> Option<JournalError> {
        self.first.take()
    }

    pub fn additional_failures(&self) -> u64 {
        self.failures.saturating_sub(1)
    }

    pub fn summary_event_fields(&self, event: Event) -> Event {
        let event = event
            .field("journal_failures_after_spawn", self.failures)
            .field("journal_failure_recoveries", self.recoveries)
            .field("journal_failure_active", self.failure_active);
        match &self.last {
            Some(last) => last.event_fields(event),
            None => event
                .field("last_journal_error_operation", "NONE")
                .field("last_journal_error_path_hex", "NONE")
                .field("last_journal_error_kind", "NONE")
                .field("last_journal_raw_os_error", "NONE")
                .field("last_journal_error_hex", "NONE"),
        }
    }
}

impl JournalErrorEvidence {
    fn capture(error: &JournalError) -> Self {
        Self {
            operation: error.operation,
            path: error.path.clone(),
            kind: error.source.kind(),
            raw_os_error: error.source.raw_os_error(),
            detail: error.source.to_string(),
        }
    }

    fn event_fields(&self, event: Event) -> Event {
        event
            .field("last_journal_error_operation", self.operation)
            .encoded_path_field("last_journal_error_path_hex", &self.path)
            .field("last_journal_error_kind", format!("{:?}", self.kind))
            .field(
                "last_journal_raw_os_error",
                self.raw_os_error
                    .map_or_else(|| "NONE".to_owned(), |value| value.to_string()),
            )
            .field(
                "last_journal_error_hex",
                crate::event::hex_bytes(self.detail.as_bytes()),
            )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PartialRecord {
    offset: u64,
    bytes: u64,
    crc32: u32,
}

fn write_recovery_record(
    file: &mut File,
    path: &Path,
    recovered: PartialRecord,
) -> Result<(), JournalError> {
    let event = Event::new(
        "journal-partial-record-separated",
        unix_ns_or_zero(),
        std::process::id(),
    )
    .encoded_path_field("journal_path_hex", path)
    .field("interrupted_record_offset", recovered.offset)
    .field("interrupted_record_bytes", recovered.bytes)
    .field(
        "interrupted_record_crc32",
        format!("{:08x}", recovered.crc32),
    );
    write_event_record(file, path, &event)
}

fn write_event_record(file: &mut File, path: &Path, event: &Event) -> Result<(), JournalError> {
    let payload = event.render();
    let record = format!("{payload}\tcrc32={:08x}\n", crc32(payload.as_bytes()));
    file.write_all(record.as_bytes())
        .map_err(|source| JournalError::io("append journal record", path, source))
}

fn synchronize(file: &File, path: &Path, operation: &'static str) -> Result<(), JournalError> {
    file.sync_data()
        .map_err(|source| JournalError::io(operation, path, source))
}

fn with_exclusive_lock<T>(
    file: &mut File,
    path: &Path,
    operation: impl FnOnce(&mut File) -> Result<T, JournalError>,
) -> Result<T, JournalError> {
    lock(file, LockOperation::Exclusive)
        .map_err(|source| JournalError::io("lock lifecycle journal", path, source))?;
    let result = operation(file);
    let unlock = lock(file, LockOperation::Unlock)
        .map_err(|source| JournalError::io("unlock lifecycle journal", path, source));
    match (result, unlock) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(value), Ok(())) => Ok(value),
    }
}

#[derive(Clone, Copy)]
enum LockOperation {
    Exclusive,
    Unlock,
}

fn lock(file: &File, operation: LockOperation) -> io::Result<()> {
    const LOCK_EX: i32 = 2;
    const LOCK_UN: i32 = 8;
    let value = match operation {
        LockOperation::Exclusive => LOCK_EX,
        LockOperation::Unlock => LOCK_UN,
    };
    loop {
        // SAFETY: `file` owns a live descriptor for the duration of this call,
        // and `flock` neither retains the descriptor nor dereferences a pointer.
        if unsafe { flock(file.as_raw_fd(), value) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

fn recover_partial_record(
    file: &mut File,
    path: &Path,
) -> Result<Option<PartialRecord>, JournalError> {
    let length = file
        .metadata()
        .map_err(|source| JournalError::io("inspect journal", path, source))?
        .len();
    if length == 0 {
        return Ok(None);
    }

    file.seek(SeekFrom::End(-1))
        .map_err(|source| JournalError::io("seek journal", path, source))?;
    let mut final_byte = [0_u8; 1];
    file.read_exact(&mut final_byte)
        .map_err(|source| JournalError::io("read journal boundary", path, source))?;
    if final_byte[0] == b'\n' {
        return Ok(None);
    }

    let offset = partial_record_offset(file, path, length)?;
    let bytes = length.saturating_sub(offset);
    let crc32 = crc32_file_range(file, path, offset, bytes)?;

    file.write_all(b"\n")
        .map_err(|source| JournalError::io("separate partial journal record", path, source))?;
    Ok(Some(PartialRecord {
        offset,
        bytes,
        crc32,
    }))
}

fn partial_record_offset(file: &mut File, path: &Path, length: u64) -> Result<u64, JournalError> {
    const SEARCH_BYTES: usize = 64 * 1024;
    let mut cursor = length;
    let mut buffer = vec![0_u8; SEARCH_BYTES];
    while cursor != 0 {
        let amount_u64 = cursor.min(SEARCH_BYTES as u64);
        let amount = amount_u64 as usize;
        cursor -= amount_u64;
        file.seek(SeekFrom::Start(cursor))
            .map_err(|source| JournalError::io("seek partial journal record", path, source))?;
        file.read_exact(&mut buffer[..amount])
            .map_err(|source| JournalError::io("read partial journal record", path, source))?;
        if let Some(index) = buffer[..amount].iter().rposition(|byte| *byte == b'\n') {
            return Ok(cursor.saturating_add(index as u64).saturating_add(1));
        }
    }
    Ok(0)
}

fn crc32_file_range(
    file: &mut File,
    path: &Path,
    offset: u64,
    bytes: u64,
) -> Result<u32, JournalError> {
    const READ_BYTES: usize = 64 * 1024;
    file.seek(SeekFrom::Start(offset))
        .map_err(|source| JournalError::io("seek partial journal checksum", path, source))?;
    let mut remaining = bytes;
    let mut state = u32::MAX;
    let mut buffer = vec![0_u8; READ_BYTES];
    while remaining != 0 {
        let amount_u64 = remaining.min(READ_BYTES as u64);
        let amount = amount_u64 as usize;
        file.read_exact(&mut buffer[..amount])
            .map_err(|source| JournalError::io("read partial journal checksum", path, source))?;
        state = crc32_update(state, &buffer[..amount]);
        remaining -= amount_u64;
    }
    Ok(!state)
}

fn crc32(bytes: &[u8]) -> u32 {
    !crc32_update(u32::MAX, bytes)
}

fn crc32_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    crc
}

fn unix_ns_or_zero() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

#[derive(Debug)]
pub struct JournalError {
    operation: &'static str,
    path: PathBuf,
    source: io::Error,
}

impl JournalError {
    fn io(operation: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self {
            operation,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for JournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "could not {} at {}: {}",
            self.operation,
            self.path.display(),
            self.source
        )
    }
}

impl std::error::Error for JournalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_JOURNAL: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn crc32_matches_the_standard_check_vector() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn failure_tracker_bounds_evidence_and_preserves_exact_counts() {
        let mut tracker = FailureTracker::new();
        tracker.record_failure(JournalError::io(
            "first operation",
            "/tmp/first-journal",
            io::Error::from_raw_os_error(28),
        ));
        tracker.record_failure(JournalError::io(
            "second operation",
            "/tmp/second-journal",
            io::Error::from_raw_os_error(5),
        ));
        tracker.record_success();
        tracker.record_success();
        tracker.record_failure(JournalError::io(
            "last operation",
            "/tmp/last-journal",
            io::Error::from_raw_os_error(13),
        ));

        assert_eq!(tracker.failures, 3);
        assert_eq!(tracker.recoveries, 1);
        assert!(tracker.failure_active);
        assert_eq!(tracker.additional_failures(), 2);
        assert_eq!(
            tracker.last.as_ref().map(|item| item.operation),
            Some("last operation")
        );

        let summary = tracker
            .summary_event_fields(Event::new("terminal", 1, 2))
            .render();
        assert!(summary.contains("\tjournal_failures_after_spawn=3\t"));
        assert!(summary.contains("\tjournal_failure_recoveries=1\t"));
        assert!(summary.contains("\tjournal_failure_active=true\t"));
        assert!(summary.contains("\tlast_journal_error_operation=last operation\t"));
        assert!(summary.contains("\tlast_journal_raw_os_error=13\t"));

        let first = tracker.take_first().expect("first error retained");
        assert!(first.to_string().contains("first operation"));
        assert_eq!(tracker.additional_failures(), 2);
        assert!(tracker.take_first().is_none());
    }

    #[test]
    fn append_separates_and_checksums_a_record_interrupted_by_process_death() {
        let sequence = NEXT_JOURNAL.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "dmc2-journal-partial-test-{}-{sequence}.tsv",
            std::process::id()
        ));
        let mut journal = Journal::open(&path).expect("open test journal");
        let interrupted = b"schema=dmc2-process-lifecycle-v1\tevent=process-sta";
        let mut external = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open simulated interrupted writer");
        external
            .write_all(interrupted)
            .expect("write simulated interrupted record");
        external.sync_data().expect("sync interrupted record");

        journal
            .append(&Event::new("surviving-event", 42, 7))
            .expect("append after interrupted writer");
        let contents = fs::read_to_string(&path).expect("read recovered journal");
        let lines = contents.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 3, "{contents}");
        assert_eq!(lines[0].as_bytes(), interrupted);
        assert!(lines[1].contains("\tevent=journal-partial-record-separated\t"));
        assert!(lines[1].contains(&format!(
            "\tinterrupted_record_bytes={}\t",
            interrupted.len()
        )));
        assert!(lines[1].contains(&format!(
            "\tinterrupted_record_crc32={:08x}\t",
            crc32(interrupted)
        )));
        assert!(lines[2].contains("\tevent=surviving-event\t"));
        fs::remove_file(path).expect("remove recovered test journal");
    }
}
