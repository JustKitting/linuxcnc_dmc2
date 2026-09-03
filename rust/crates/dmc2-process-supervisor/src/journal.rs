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

        let recovered_partial_record =
            with_exclusive_lock(&mut file, path, |file| separate_partial_record(file, path))?;
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
        let payload = event.render();
        let record = format!("{payload}\tcrc32={:08x}\n", crc32(payload.as_bytes()));
        with_exclusive_lock(&mut self.file, &self.path, |file| {
            file.write_all(record.as_bytes())
                .map_err(|source| JournalError::io("append journal record", &self.path, source))?;
            file.sync_data().map_err(|source| {
                JournalError::io("synchronize journal record", &self.path, source)
            })
        })
    }
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

fn separate_partial_record(file: &mut File, path: &Path) -> Result<bool, JournalError> {
    let length = file
        .metadata()
        .map_err(|source| JournalError::io("inspect journal", path, source))?
        .len();
    if length == 0 {
        return Ok(false);
    }

    file.seek(SeekFrom::End(-1))
        .map_err(|source| JournalError::io("seek journal", path, source))?;
    let mut final_byte = [0_u8; 1];
    file.read_exact(&mut final_byte)
        .map_err(|source| JournalError::io("read journal boundary", path, source))?;
    if final_byte[0] == b'\n' {
        return Ok(false);
    }

    file.write_all(b"\n")
        .map_err(|source| JournalError::io("separate partial journal record", path, source))?;
    file.sync_data()
        .map_err(|source| JournalError::io("synchronize journal repair", path, source))?;
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

    #[test]
    fn crc32_matches_the_standard_check_vector() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }
}
