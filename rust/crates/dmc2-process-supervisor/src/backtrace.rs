use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::event::{hex_bytes, signal_name, Event, EventTime};

const FILE_TIMESTAMP_TOLERANCE: Duration = Duration::from_secs(1);

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

    pub fn event_fields(&self, event: Event) -> Event {
        let event = event.field(
            "backtrace_timestamp_tolerance_ns",
            FILE_TIMESTAMP_TOLERANCE.as_nanos(),
        );
        match self {
            Self::NotApplicable => event.field("backtrace_state", "not-applicable"),
            Self::Absent { source } => event
                .field("backtrace_state", "absent")
                .encoded_path_field("backtrace_source_hex", source),
            Self::Captured {
                source,
                durable_copy,
                reported_signal,
            } => event
                .field("backtrace_state", "captured")
                .encoded_path_field("backtrace_source_hex", source)
                .encoded_path_field("backtrace_copy_hex", durable_copy)
                .field("backtrace_signal", optional_i32(*reported_signal))
                .field(
                    "backtrace_signal_name",
                    reported_signal.map(signal_name).unwrap_or("NONE"),
                ),
            Self::Rejected { source, reason } => event
                .field("backtrace_state", "rejected")
                .encoded_path_field("backtrace_source_hex", source)
                .field("backtrace_rejection", reason),
            Self::CaptureFailed {
                source,
                operation,
                error,
            } => event
                .field("backtrace_state", "capture-failed")
                .encoded_path_field("backtrace_source_hex", source)
                .field("backtrace_operation", operation)
                .field("backtrace_error_kind", format!("{:?}", error.kind()))
                .field("backtrace_raw_os_error", optional_i32(error.raw_os_error()))
                .field(
                    "backtrace_error_hex",
                    hex_bytes(error.to_string().as_bytes()),
                ),
        }
    }
}

pub fn capture(
    journal_path: &Path,
    child_pid: u32,
    process_not_before: SystemTime,
    exit_unix_ns: impl Into<EventTime>,
) -> BacktraceEvidence {
    let source = PathBuf::from(format!("/tmp/backtrace.{child_pid}"));
    let exit_unix_ns = match exit_unix_ns.into() {
        EventTime::Captured(value) => value,
        error @ EventTime::BeforeUnixEpoch { .. } => {
            return BacktraceEvidence::CaptureFailed {
                source,
                operation: "capture valid exit timestamp",
                error: io::Error::other(format!("{error}; correct system time and relaunch through Applications")),
            };
        }
    };
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
        Ok(modified)
            if modified
                .checked_add(FILE_TIMESTAMP_TOLERANCE)
                .is_some_and(|latest_credible_time| latest_credible_time < process_not_before) =>
        {
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

    let Some(parent) = journal_path.parent() else {
        return BacktraceEvidence::CaptureFailed {
            source,
            operation: "resolve configured journal directory",
            error: io::Error::other(format!("journal path {} has no parent; correct its configured path before relaunching", journal_path.display())),
        };
    };
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
        if let Some(raw_signal) = fields
            .iter()
            .find_map(|field| field.strip_prefix("signal="))
        {
            let signal = raw_signal.parse::<i32>().map_err(|error| {
                HeaderError::Io(io::Error::new(io::ErrorKind::InvalidData,
                    format!("invalid backtrace signal {raw_signal:?}: {error}; retain the malformed backtrace and correct its writer")))
            })?;
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
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput,
            format!("backtrace destination {} has no parent directory", destination.display()))
    })?;
    File::open(parent)?.sync_all()
}

enum HeaderError {
    NoMatchingPid,
    Io(io::Error),
}

fn optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "NONE".to_owned(), |value| value.to_string())
}
