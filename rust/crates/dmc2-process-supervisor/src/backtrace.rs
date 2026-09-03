use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug)]
pub enum BacktraceEvidence {
    NotApplicable,
    Absent {
        source: PathBuf,
    },
    Captured {
        source: PathBuf,
        durable_copy: PathBuf,
        reported_signal: Option<i32>,
    },
    Rejected {
        source: PathBuf,
        reason: &'static str,
    },
    CaptureFailed {
        source: PathBuf,
        operation: &'static str,
        error: io::Error,
    },
}

impl BacktraceEvidence {
    pub fn reported_signal(&self) -> Option<i32> {
        match self {
            Self::Captured {
                reported_signal, ..
            } => *reported_signal,
            _ => None,
        }
    }
}

pub fn capture(
    journal_path: &Path,
    child_pid: u32,
    launched_at: SystemTime,
    exit_unix_ns: u128,
) -> BacktraceEvidence {
    let source = PathBuf::from(format!("/tmp/backtrace.{child_pid}"));
    let metadata = match fs::symlink_metadata(&source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return BacktraceEvidence::Absent { source };
        }
        Err(error) => {
            return BacktraceEvidence::CaptureFailed {
                source,
                operation: "inspect LinuxCNC backtrace",
                error,
            };
        }
    };
    if !metadata.file_type().is_file() {
        return BacktraceEvidence::Rejected {
            source,
            reason: "not-a-regular-file",
        };
    }

    match metadata.modified() {
        Ok(modified) if modified < launched_at => {
            return BacktraceEvidence::Rejected {
                source,
                reason: "predates-supervised-process",
            };
        }
        Err(error) => {
            return BacktraceEvidence::CaptureFailed {
                source,
                operation: "read LinuxCNC backtrace timestamp",
                error,
            };
        }
        _ => {}
    }

    let reported_signal = match read_header_signal(&source, child_pid) {
        Ok(reported_signal) => reported_signal,
        Err(HeaderError::NoMatchingPid) => {
            return BacktraceEvidence::Rejected {
                source,
                reason: "no-matching-pid-header",
            };
        }
        Err(HeaderError::Io(error)) => {
            return BacktraceEvidence::CaptureFailed {
                source,
                operation: "read LinuxCNC backtrace header",
                error,
            };
        }
    };

    let parent = journal_path.parent().unwrap_or_else(|| Path::new("."));
    let durable_copy = parent.join(format!("process-backtrace-{child_pid}-{exit_unix_ns}.txt"));
    if let Err(error) = copy_and_sync(&source, &durable_copy) {
        return BacktraceEvidence::CaptureFailed {
            source,
            operation: "preserve LinuxCNC backtrace",
            error,
        };
    }

    BacktraceEvidence::Captured {
        source,
        durable_copy,
        reported_signal,
    }
}

fn read_header_signal(path: &Path, expected_pid: u32) -> Result<Option<i32>, HeaderError> {
    let file = File::open(path).map_err(HeaderError::Io)?;
    let pid_marker = format!("pid={expected_pid}");
    let mut found_pid = false;
    let mut reported_signal = None;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).map_err(HeaderError::Io)? == 0 {
            break;
        }
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        if !fields.iter().any(|field| *field == pid_marker.as_str()) {
            continue;
        }
        found_pid = true;
        if let Some(signal) = fields
            .iter()
            .find_map(|field| field.strip_prefix("signal="))
            .and_then(|value| value.parse::<i32>().ok())
        {
            reported_signal = Some(signal);
        }
    }
    found_pid
        .then_some(reported_signal)
        .ok_or(HeaderError::NoMatchingPid)
}

fn copy_and_sync(source: &Path, destination: &Path) -> io::Result<()> {
    let mut source_file = File::open(source)?;
    let mut destination_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)?;
    io::copy(&mut source_file, &mut destination_file)?;
    destination_file.flush()?;
    destination_file.sync_all()?;
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    File::open(parent)?.sync_all()
}

enum HeaderError {
    NoMatchingPid,
    Io(io::Error),
}
