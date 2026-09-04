use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use crate::event::Event;

pub struct Journal {
    file: File,
    path: PathBuf,
    recovered_partial_record: bool,
}
pub struct FailureTracker {
    first: Option<JournalError>,
    failures: u64,
    recoveries: u64,
    active: bool,
}

impl Journal {
    pub fn open(path: &Path) -> Result<Self, JournalError> {
        let parent = path.parent().filter(|path| !path.as_os_str().is_empty());
        if let Some(parent) = parent {
            fs::create_dir_all(parent)
                .map_err(|source| JournalError::new("create journal directory", parent, source))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .mode(0o600)
            .open(path)
            .map_err(|source| JournalError::new("open journal", path, source))?;
        if let Some(parent) = parent {
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|source| {
                    JournalError::new("synchronize journal directory", parent, source)
                })?;
        }
        let recovered_partial_record =
            with_lock(&mut file, path, |file| separate_partial_record(file, path))?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
            recovered_partial_record,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn recovered_partial_record(&self) -> bool {
        self.recovered_partial_record
    }

    pub fn append(&mut self, event: &Event) -> Result<(), JournalError> {
        let payload = event.render();
        let record = format!("{payload}\tcrc32={:08x}\n", crc32(payload.as_bytes()));
        with_lock(&mut self.file, &self.path, |file| {
            separate_partial_record(file, &self.path)?;
            file.write_all(record.as_bytes())
                .map_err(|source| JournalError::new("append journal record", &self.path, source))?;
            file.sync_data().map_err(|source| {
                JournalError::new("synchronize journal record", &self.path, source)
            })
        })
    }
}

impl FailureTracker {
    pub fn new() -> Self {
        Self {
            first: None,
            failures: 0,
            recoveries: 0,
            active: false,
        }
    }
    pub fn record_failure(&mut self, error: JournalError) {
        self.failures = self.failures.saturating_add(1);
        self.active = true;
        if self.first.is_none() {
            self.first = Some(error);
        }
    }
    pub fn record_success(&mut self) -> bool {
        if self.active {
            self.active = false;
            self.recoveries = self.recoveries.saturating_add(1);
            true
        } else {
            false
        }
    }
    pub fn take_first(&mut self) -> Option<JournalError> {
        self.first.take()
    }
    pub fn failures(&self) -> u64 {
        self.failures
    }
    pub fn additional_failures(&self) -> u64 {
        self.failures.saturating_sub(1)
    }
    pub fn event_fields(&self, event: Event) -> Event {
        event
            .field("journal_failures_after_spawn", self.failures)
            .field("journal_failure_recoveries", self.recoveries)
            .field("journal_failure_active", self.active)
    }
}

fn with_lock<T>(
    file: &mut File,
    path: &Path,
    operation: impl FnOnce(&mut File) -> Result<T, JournalError>,
) -> Result<T, JournalError> {
    lock(file, LOCK_EX).map_err(|source| JournalError::new("lock journal", path, source))?;
    let result = operation(file);
    let unlock =
        lock(file, LOCK_UN).map_err(|source| JournalError::new("unlock journal", path, source));
    match (result, unlock) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(value), Ok(())) => Ok(value),
    }
}

const LOCK_EX: i32 = 2;
const LOCK_UN: i32 = 8;

fn lock(file: &File, operation: i32) -> io::Result<()> {
    loop {
        // SAFETY: the descriptor is valid for this call and flock retains no pointer.
        if unsafe { flock(file.as_raw_fd(), operation) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn separate_partial_record(file: &mut File, path: &Path) -> Result<bool, JournalError> {
    let length = file
        .metadata()
        .map_err(|source| JournalError::new("inspect journal", path, source))?
        .len();
    if length == 0 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-1))
        .map_err(|source| JournalError::new("seek journal", path, source))?;
    let mut final_byte = [0_u8; 1];
    file.read_exact(&mut final_byte)
        .map_err(|source| JournalError::new("read journal boundary", path, source))?;
    if final_byte[0] == b'\n' {
        return Ok(false);
    }
    file.write_all(b"\n")
        .map_err(|source| JournalError::new("separate interrupted journal record", path, source))?;
    file.sync_data()
        .map_err(|source| JournalError::new("synchronize journal repair", path, source))?;
    Ok(true)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

#[derive(Debug)]
pub struct JournalError {
    operation: &'static str,
    path: PathBuf,
    source: io::Error,
}

impl JournalError {
    fn new(operation: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self {
            operation,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for JournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "could not {} at {}: {}; recovery: correct the log path or storage and relaunch from the desktop application", self.operation, self.path.display(), self.source)
    }
}

impl std::error::Error for JournalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}
