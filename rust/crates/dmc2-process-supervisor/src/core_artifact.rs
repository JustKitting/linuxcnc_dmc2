use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::event::{hex_bytes, Event};

const CORE_PATTERN_PATH: &str = "/proc/sys/kernel/core_pattern";
const CORE_USES_PID_PATH: &str = "/proc/sys/kernel/core_uses_pid";
const O_NOFOLLOW: i32 = 0x0002_0000;
const FILE_TIMESTAMP_TOLERANCE: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct WorkingDirectoryEvidence {
    source: &'static str,
    result: Result<PathBuf, IoFailure>,
}

impl WorkingDirectoryEvidence {
    pub fn for_process(pid: u32) -> Self {
        Self::capture("proc-cwd-after-process-observation", || {
            fs::read_link(format!("/proc/{pid}/cwd"))
        })
    }

    pub fn for_supervisor() -> Self {
        Self::capture("supervisor-current-directory", std::env::current_dir)
    }

    fn capture(source: &'static str, operation: impl FnOnce() -> io::Result<PathBuf>) -> Self {
        Self {
            source,
            result: operation().map_err(IoFailure::capture),
        }
    }

    fn path(&self) -> Option<&Path> {
        self.result.as_deref().ok()
    }
}

#[derive(Debug)]
pub enum CoreArtifactEvidence {
    NotDumped,
    PolicyUnavailable {
        operation: &'static str,
        path: &'static str,
        error: IoFailure,
    },
    ExternalHandler {
        policy: CorePolicy,
    },
    DisabledByPolicy {
        policy: CorePolicy,
    },
    UnsupportedTemplate {
        policy: CorePolicy,
        reason: &'static str,
    },
    LocationUnavailable {
        policy: CorePolicy,
        primary: WorkingDirectoryEvidence,
        fallback: WorkingDirectoryEvidence,
    },
    Absent {
        location: CoreLocation,
    },
    Rejected {
        location: CoreLocation,
        reason: &'static str,
        identity: FileIdentity,
    },
    CaptureFailed {
        location: CoreLocation,
        operation: &'static str,
        destination: Option<PathBuf>,
        error: IoFailure,
    },
    Captured {
        location: CoreLocation,
        destination: PathBuf,
        identity: FileIdentity,
        copied_bytes: u64,
    },
}

impl CoreArtifactEvidence {
    pub fn event_fields(&self, event: Event) -> Event {
        match self {
            Self::NotDumped => event.field("core_artifact_state", "not-dumped"),
            Self::PolicyUnavailable {
                operation,
                path,
                error,
            } => io_failure_fields(
                event
                    .field("core_artifact_state", "policy-unavailable")
                    .field("core_artifact_operation", operation)
                    .field("core_policy_path", path),
                error,
            ),
            Self::ExternalHandler { policy } => policy.event_fields(
                event
                    .field("core_artifact_state", "external-handler")
                    .field(
                        "core_artifact_detail",
                        "kernel-core-pattern-pipes-to-external-handler",
                    ),
            ),
            Self::DisabledByPolicy { policy } => policy.event_fields(
                event
                    .field("core_artifact_state", "disabled-by-policy")
                    .field(
                        "core_artifact_detail",
                        "empty-pattern-and-core-uses-pid-disabled",
                    ),
            ),
            Self::UnsupportedTemplate { policy, reason } => policy.event_fields(
                event
                    .field("core_artifact_state", "unsupported-template")
                    .field("core_artifact_detail", reason),
            ),
            Self::LocationUnavailable {
                policy,
                primary,
                fallback,
            } => {
                let event = policy.event_fields(
                    event
                        .field("core_artifact_state", "location-unavailable")
                        .field("core_artifact_detail", "relative-pattern-without-known-cwd"),
                );
                working_directory_fields(event, primary, fallback, None)
            }
            Self::Absent { location } => location
                .event_fields(event.field("core_artifact_state", "absent-after-core-signalled")),
            Self::Rejected {
                location,
                reason,
                identity,
            } => {
                let event = location.event_fields(
                    event
                        .field("core_artifact_state", "rejected")
                        .field("core_artifact_detail", reason),
                );
                identity.event_fields(event)
            }
            Self::CaptureFailed {
                location,
                operation,
                destination,
                error,
            } => {
                let mut event = location.event_fields(
                    event
                        .field("core_artifact_state", "capture-failed")
                        .field("core_artifact_operation", operation),
                );
                if let Some(destination) = destination {
                    event = event.encoded_path_field("core_artifact_copy_hex", destination);
                }
                io_failure_fields(event, error)
            }
            Self::Captured {
                location,
                destination,
                identity,
                copied_bytes,
            } => {
                let event = location
                    .event_fields(event.field("core_artifact_state", "captured"))
                    .encoded_path_field("core_artifact_copy_hex", destination)
                    .field("core_artifact_copied_bytes", copied_bytes);
                identity.event_fields(event)
            }
        }
    }
}

pub fn capture(
    journal_path: &Path,
    child_pid: u32,
    core_dumped: bool,
    primary_cwd: &WorkingDirectoryEvidence,
    fallback_cwd: &WorkingDirectoryEvidence,
    process_not_before: SystemTime,
    exit_unix_ns: u128,
) -> CoreArtifactEvidence {
    if !core_dumped {
        return CoreArtifactEvidence::NotDumped;
    }

    let policy = match CorePolicy::read() {
        Ok(policy) => policy,
        Err(error) => return error,
    };
    if policy.pattern.first() == Some(&b'|') {
        return CoreArtifactEvidence::ExternalHandler { policy };
    }
    if policy.pattern.is_empty() && !policy.uses_pid {
        return CoreArtifactEvidence::DisabledByPolicy { policy };
    }
    if policy.pattern.contains(&b'%') {
        return CoreArtifactEvidence::UnsupportedTemplate {
            policy,
            reason: "percent-expansion-requires-kernel-resolved-name",
        };
    }

    let mut name = policy.pattern.clone();
    if policy.uses_pid {
        name.extend_from_slice(format!(".{child_pid}").as_bytes());
    }
    let configured = PathBuf::from(OsString::from_vec(name));
    let (source, selected_cwd) = if configured.is_absolute() {
        (configured, None)
    } else if let Some(cwd) = primary_cwd.path() {
        (cwd.join(configured), Some(primary_cwd.source))
    } else if let Some(cwd) = fallback_cwd.path() {
        (cwd.join(configured), Some(fallback_cwd.source))
    } else {
        return CoreArtifactEvidence::LocationUnavailable {
            policy,
            primary: primary_cwd.clone(),
            fallback: fallback_cwd.clone(),
        };
    };
    let location = CoreLocation {
        policy,
        primary: primary_cwd.clone(),
        fallback: fallback_cwd.clone(),
        selected_cwd,
        source,
        process_not_before_unix_ns: process_not_before
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_nanos()),
    };

    let metadata = match fs::symlink_metadata(&location.source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return CoreArtifactEvidence::Absent { location };
        }
        Err(error) => {
            return CoreArtifactEvidence::CaptureFailed {
                location,
                operation: "inspect-kernel-core",
                destination: None,
                error: IoFailure::capture(error),
            };
        }
    };
    let identity = FileIdentity::from_metadata(&metadata);
    if !metadata.file_type().is_file() {
        return CoreArtifactEvidence::Rejected {
            location,
            reason: "not-a-regular-file",
            identity,
        };
    }
    match metadata.modified() {
        Ok(modified)
            if modified
                .checked_add(FILE_TIMESTAMP_TOLERANCE)
                .is_some_and(|latest_credible_time| latest_credible_time < process_not_before) =>
        {
            return CoreArtifactEvidence::Rejected {
                location,
                reason: "predates-supervised-session",
                identity,
            };
        }
        Ok(_) => {}
        Err(error) => {
            return CoreArtifactEvidence::CaptureFailed {
                location,
                operation: "read-kernel-core-timestamp",
                destination: None,
                error: IoFailure::capture(error),
            };
        }
    }

    let parent = journal_path.parent().unwrap_or_else(|| Path::new("."));
    let destination = parent.join(format!("process-core-{child_pid}-{exit_unix_ns}.core"));
    match copy_and_sync(&location.source, &destination, identity) {
        Ok(copied_bytes) => CoreArtifactEvidence::Captured {
            location,
            destination,
            identity,
            copied_bytes,
        },
        Err((operation, error)) => CoreArtifactEvidence::CaptureFailed {
            location,
            operation,
            destination: Some(destination),
            error: IoFailure::capture(error),
        },
    }
}

#[derive(Debug)]
pub struct CorePolicy {
    pattern: Vec<u8>,
    uses_pid: bool,
}

impl CorePolicy {
    fn read() -> Result<Self, CoreArtifactEvidence> {
        let pattern = fs::read(CORE_PATTERN_PATH).map_err(|error| {
            CoreArtifactEvidence::PolicyUnavailable {
                operation: "read-core-pattern",
                path: CORE_PATTERN_PATH,
                error: IoFailure::capture(error),
            }
        })?;
        let pattern = strip_line_ending(&pattern).to_vec();
        let uses_pid = fs::read(CORE_USES_PID_PATH).map_err(|error| {
            CoreArtifactEvidence::PolicyUnavailable {
                operation: "read-core-uses-pid",
                path: CORE_USES_PID_PATH,
                error: IoFailure::capture(error),
            }
        })?;
        let uses_pid = trim_ascii_whitespace(&uses_pid);
        let uses_pid = std::str::from_utf8(uses_pid)
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .map(|value| value != 0)
            .ok_or_else(|| CoreArtifactEvidence::PolicyUnavailable {
                operation: "parse-core-uses-pid",
                path: CORE_USES_PID_PATH,
                error: IoFailure::capture(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid core_uses_pid bytes: {}", hex_bytes(uses_pid)),
                )),
            })?;
        Ok(Self { pattern, uses_pid })
    }

    fn event_fields(&self, event: Event) -> Event {
        event
            .field("core_pattern_hex", hex_bytes(&self.pattern))
            .field("core_uses_pid", self.uses_pid)
    }
}

#[derive(Debug)]
pub struct CoreLocation {
    policy: CorePolicy,
    primary: WorkingDirectoryEvidence,
    fallback: WorkingDirectoryEvidence,
    selected_cwd: Option<&'static str>,
    source: PathBuf,
    process_not_before_unix_ns: Option<u128>,
}

impl CoreLocation {
    fn event_fields(&self, event: Event) -> Event {
        let event = self
            .policy
            .event_fields(event)
            .field(
                "core_artifact_timestamp_tolerance_ns",
                FILE_TIMESTAMP_TOLERANCE.as_nanos(),
            )
            .field(
                "core_artifact_process_not_before_unix_ns",
                self.process_not_before_unix_ns
                    .map_or_else(|| "NONE".to_owned(), |value| value.to_string()),
            )
            .encoded_path_field("core_artifact_source_hex", &self.source);
        working_directory_fields(event, &self.primary, &self.fallback, self.selected_cwd)
    }
}

#[derive(Debug, Clone)]
pub struct IoFailure {
    kind: String,
    raw_os_error: Option<i32>,
    message_hex: String,
}

impl IoFailure {
    fn capture(error: io::Error) -> Self {
        Self {
            kind: format!("{:?}", error.kind()),
            raw_os_error: error.raw_os_error(),
            message_hex: hex_bytes(error.to_string().as_bytes()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FileIdentity {
    device: u64,
    inode: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
}

impl FileIdentity {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            uid: metadata.uid(),
            gid: metadata.gid(),
            size: metadata.size(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
        }
    }

    fn event_fields(self, event: Event) -> Event {
        event
            .field("core_artifact_device", self.device)
            .field("core_artifact_inode", self.inode)
            .field("core_artifact_mode", self.mode)
            .field("core_artifact_uid", self.uid)
            .field("core_artifact_gid", self.gid)
            .field("core_artifact_size", self.size)
            .field("core_artifact_mtime_seconds", self.modified_seconds)
            .field("core_artifact_mtime_nanoseconds", self.modified_nanoseconds)
    }
}

fn copy_and_sync(
    source: &Path,
    destination: &Path,
    expected: FileIdentity,
) -> Result<u64, (&'static str, io::Error)> {
    let mut source_file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(source)
        .map_err(|error| ("open-kernel-core-without-following-links", error))?;
    let opened = FileIdentity::from_metadata(
        &source_file
            .metadata()
            .map_err(|error| ("inspect-open-kernel-core", error))?,
    );
    if opened.device != expected.device || opened.inode != expected.inode {
        return Err((
            "verify-open-kernel-core-identity",
            io::Error::other("kernel core identity changed between inspection and open"),
        ));
    }
    let mut destination_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)
        .map_err(|error| ("create-durable-core-copy", error))?;
    let copied = io::copy(&mut source_file, &mut destination_file)
        .map_err(|error| ("copy-kernel-core", error))?;
    destination_file
        .flush()
        .map_err(|error| ("flush-durable-core-copy", error))?;
    destination_file
        .sync_all()
        .map_err(|error| ("synchronize-durable-core-copy", error))?;
    let copied_size = destination_file
        .metadata()
        .map_err(|error| ("inspect-durable-core-copy", error))?
        .len();
    if copied != expected.size || copied_size != expected.size {
        return Err((
            "verify-durable-core-copy-size",
            io::Error::other(format!(
                "expected {} bytes, copied {copied}, destination contains {copied_size}",
                expected.size
            )),
        ));
    }
    File::open(destination.parent().unwrap_or_else(|| Path::new(".")))
        .and_then(|directory| directory.sync_all())
        .map_err(|error| ("synchronize-core-copy-directory", error))?;
    Ok(copied)
}

fn working_directory_fields(
    mut event: Event,
    primary: &WorkingDirectoryEvidence,
    fallback: &WorkingDirectoryEvidence,
    selected: Option<&'static str>,
) -> Event {
    event = one_working_directory_fields(event, WorkingDirectorySlot::Primary, primary);
    event = one_working_directory_fields(event, WorkingDirectorySlot::Fallback, fallback);
    event.field("core_cwd_selected_source", selected.unwrap_or("NONE"))
}

#[derive(Clone, Copy)]
enum WorkingDirectorySlot {
    Primary,
    Fallback,
}

fn one_working_directory_fields(
    event: Event,
    slot: WorkingDirectorySlot,
    evidence: &WorkingDirectoryEvidence,
) -> Event {
    let (state_field, source_field, path_field, kind_field, errno_field, error_field) = match slot {
        WorkingDirectorySlot::Primary => (
            "core_cwd_primary_state",
            "core_cwd_primary_source",
            "core_cwd_primary_path_hex",
            "core_cwd_primary_error_kind",
            "core_cwd_primary_raw_os_error",
            "core_cwd_primary_error_hex",
        ),
        WorkingDirectorySlot::Fallback => (
            "core_cwd_fallback_state",
            "core_cwd_fallback_source",
            "core_cwd_fallback_path_hex",
            "core_cwd_fallback_error_kind",
            "core_cwd_fallback_raw_os_error",
            "core_cwd_fallback_error_hex",
        ),
    };
    let event = event.field(source_field, evidence.source);
    match &evidence.result {
        Ok(path) => event
            .field(state_field, "captured")
            .encoded_path_field(path_field, path),
        Err(error) => event
            .field(state_field, "unavailable")
            .field(kind_field, &error.kind)
            .field(
                errno_field,
                error
                    .raw_os_error
                    .map_or_else(|| "NONE".to_owned(), |value| value.to_string()),
            )
            .field(error_field, &error.message_hex),
    }
}

fn io_failure_fields(event: Event, error: &IoFailure) -> Event {
    event
        .field("core_artifact_error_kind", &error.kind)
        .field(
            "core_artifact_raw_os_error",
            error
                .raw_os_error
                .map_or_else(|| "NONE".to_owned(), |value| value.to_string()),
        )
        .field("core_artifact_error_hex", &error.message_hex)
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

fn strip_line_ending(value: &[u8]) -> &[u8] {
    let value = value.strip_suffix(b"\n").unwrap_or(value);
    value.strip_suffix(b"\r").unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_pattern_keeps_literal_whitespace_and_removes_only_the_line_ending() {
        assert_eq!(strip_line_ending(b" core name \n"), b" core name ");
        assert_eq!(strip_line_ending(b"core\r\n"), b"core");
    }
}
