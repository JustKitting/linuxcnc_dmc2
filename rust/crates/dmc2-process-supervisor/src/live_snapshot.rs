use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::event::{hex_bytes, Event};
use crate::process;

const MAX_SMALL_PROC_FILE_BYTES: usize = 256 * 1024;
const MAX_MAPS_BYTES: usize = 1024 * 1024;
const MAX_FD_CATALOG_BYTES: usize = 512 * 1024;
const MAX_TASK_CATALOG_BYTES: usize = 64 * 1024;

const PROC_FILES: &[(&str, &str, usize)] = &[
    (
        "last_live_proc_status_hex",
        "status",
        MAX_SMALL_PROC_FILE_BYTES,
    ),
    (
        "last_live_proc_limits_hex",
        "limits",
        MAX_SMALL_PROC_FILE_BYTES,
    ),
    ("last_live_proc_io_hex", "io", MAX_SMALL_PROC_FILE_BYTES),
    (
        "last_live_proc_sched_hex",
        "sched",
        MAX_SMALL_PROC_FILE_BYTES,
    ),
    (
        "last_live_proc_schedstat_hex",
        "schedstat",
        MAX_SMALL_PROC_FILE_BYTES,
    ),
    (
        "last_live_proc_wchan_hex",
        "wchan",
        MAX_SMALL_PROC_FILE_BYTES,
    ),
    (
        "last_live_proc_syscall_hex",
        "syscall",
        MAX_SMALL_PROC_FILE_BYTES,
    ),
    (
        "last_live_proc_statm_hex",
        "statm",
        MAX_SMALL_PROC_FILE_BYTES,
    ),
    (
        "last_live_proc_cgroup_hex",
        "cgroup",
        MAX_SMALL_PROC_FILE_BYTES,
    ),
    (
        "last_live_proc_cmdline_hex",
        "cmdline",
        MAX_SMALL_PROC_FILE_BYTES,
    ),
    ("last_live_proc_comm_hex", "comm", MAX_SMALL_PROC_FILE_BYTES),
    (
        "last_live_proc_oom_score_hex",
        "oom_score",
        MAX_SMALL_PROC_FILE_BYTES,
    ),
    (
        "last_live_proc_oom_score_adj_hex",
        "oom_score_adj",
        MAX_SMALL_PROC_FILE_BYTES,
    ),
    ("last_live_proc_maps_hex", "maps", MAX_MAPS_BYTES),
];

#[derive(Debug)]
pub struct Tracker {
    period: Duration,
    next_due: Instant,
    attempts: u64,
    successes: u64,
    last_attempt: Attempt,
    last_live: Option<Snapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTransition {
    Unavailable,
    Restored,
}

impl CaptureTransition {
    pub const fn direct_event_name(self) -> &'static str {
        match self {
            Self::Unavailable => "live-snapshot-unavailable",
            Self::Restored => "live-snapshot-restored",
        }
    }

    pub const fn session_event_name(self) -> &'static str {
        match self {
            Self::Unavailable => "session-child-live-snapshot-unavailable",
            Self::Restored => "session-child-live-snapshot-restored",
        }
    }
}

impl Tracker {
    pub fn deferred(period_ms: u64) -> Self {
        let period = Duration::from_millis(period_ms);
        Self {
            period,
            next_due: Instant::now(),
            attempts: 0,
            successes: 0,
            last_attempt: Attempt::Never,
            last_live: None,
        }
    }

    pub fn set_period_ms(&mut self, period_ms: u64) {
        self.period = Duration::from_millis(period_ms);
        self.next_due = Instant::now() + self.period;
    }

    pub fn capture_if_due(&mut self, pid: u32) -> Option<CaptureTransition> {
        if Instant::now() >= self.next_due {
            self.capture_now(pid)
        } else {
            None
        }
    }

    pub fn capture_now(&mut self, pid: u32) -> Option<CaptureTransition> {
        let previously_failed = matches!(self.last_attempt, Attempt::Failed(_));
        self.attempts = self.attempts.saturating_add(1);
        let sequence = self.attempts;
        let transition = match Snapshot::capture(pid, sequence) {
            Ok(snapshot) => {
                self.successes = self.successes.saturating_add(1);
                self.last_attempt = Attempt::Captured {
                    unix_ns: snapshot.finished_unix_ns,
                    process_state: snapshot.process_state.clone(),
                };
                self.last_live = Some(snapshot);
                previously_failed.then_some(CaptureTransition::Restored)
            }
            Err(failure) => {
                self.last_attempt = Attempt::Failed(failure);
                (!previously_failed).then_some(CaptureTransition::Unavailable)
            }
        };
        self.next_due = Instant::now() + self.period;
        transition
    }

    pub fn summary_event_fields(&self, event: Event) -> Event {
        let event = event
            .field("live_snapshot_period_ms", self.period.as_millis())
            .field("live_snapshot_attempts", self.attempts)
            .field("live_snapshot_successes", self.successes);
        let event = self.last_attempt.event_fields(event);
        match &self.last_live {
            Some(snapshot) => snapshot.summary_event_fields(event),
            None => event
                .field("last_live_snapshot_state", "never-captured")
                .field("last_live_snapshot_sequence", "NONE")
                .field("last_live_snapshot_started_unix_ns", "NONE")
                .field("last_live_snapshot_finished_unix_ns", "NONE")
                .field("last_live_snapshot_age_ns", "NONE")
                .field("last_live_snapshot_process_state", "NONE")
                .field("last_live_snapshot_fd_entries_seen", "NONE"),
        }
    }

    pub fn full_event_fields(&self, event: Event) -> Event {
        let event = self.summary_event_fields(event);
        match &self.last_live {
            Some(snapshot) => snapshot.full_event_fields(event),
            None => event.field("last_live_snapshot_payload_state", "unavailable"),
        }
    }
}

#[derive(Debug)]
enum Attempt {
    Never,
    Captured {
        unix_ns: u128,
        process_state: String,
    },
    Failed(CaptureFailure),
}

impl Attempt {
    fn event_fields(&self, event: Event) -> Event {
        match self {
            Self::Never => event
                .field("live_snapshot_last_attempt_state", "never-attempted")
                .field("live_snapshot_last_attempt_unix_ns", "NONE")
                .field("live_snapshot_last_attempt_process_state", "NONE")
                .field("live_snapshot_last_attempt_error_kind", "NONE")
                .field("live_snapshot_last_attempt_raw_os_error", "NONE")
                .field("live_snapshot_last_attempt_detail_hex", "NONE"),
            Self::Captured {
                unix_ns,
                process_state,
            } => event
                .field("live_snapshot_last_attempt_state", "captured-live")
                .field("live_snapshot_last_attempt_unix_ns", unix_ns)
                .field("live_snapshot_last_attempt_process_state", process_state)
                .field("live_snapshot_last_attempt_error_kind", "NONE")
                .field("live_snapshot_last_attempt_raw_os_error", "NONE")
                .field("live_snapshot_last_attempt_detail_hex", "NONE"),
            Self::Failed(failure) => failure.event_fields(event),
        }
    }
}

#[derive(Debug)]
struct CaptureFailure {
    unix_ns: u128,
    state: &'static str,
    process_state: Option<String>,
    error_kind: Option<String>,
    raw_os_error: Option<i32>,
    detail: Vec<u8>,
}

impl CaptureFailure {
    fn io(unix_ns: u128, state: &'static str, error: &io::Error) -> Self {
        Self {
            unix_ns,
            state,
            process_state: None,
            error_kind: Some(format!("{:?}", error.kind())),
            raw_os_error: error.raw_os_error(),
            detail: error.to_string().into_bytes(),
        }
    }

    fn invalid(unix_ns: u128, state: &'static str, detail: impl Into<Vec<u8>>) -> Self {
        Self {
            unix_ns,
            state,
            process_state: None,
            error_kind: None,
            raw_os_error: None,
            detail: detail.into(),
        }
    }

    fn terminal(unix_ns: u128, state: &'static str, process_state: String, stat: Vec<u8>) -> Self {
        Self {
            unix_ns,
            state,
            process_state: Some(process_state),
            error_kind: None,
            raw_os_error: None,
            detail: stat,
        }
    }

    fn event_fields(&self, event: Event) -> Event {
        event
            .field("live_snapshot_last_attempt_state", self.state)
            .field("live_snapshot_last_attempt_unix_ns", self.unix_ns)
            .field(
                "live_snapshot_last_attempt_process_state",
                self.process_state.as_deref().unwrap_or("NONE"),
            )
            .field(
                "live_snapshot_last_attempt_error_kind",
                self.error_kind.as_deref().unwrap_or("NONE"),
            )
            .field(
                "live_snapshot_last_attempt_raw_os_error",
                optional_i32(self.raw_os_error),
            )
            .field(
                "live_snapshot_last_attempt_detail_hex",
                hex_bytes(&self.detail),
            )
    }
}

#[derive(Debug)]
struct Snapshot {
    pid: u32,
    sequence: u64,
    started_unix_ns: u128,
    finished_unix_ns: u128,
    capture_duration_ns: u128,
    captured_at: Instant,
    process_state: String,
    start_time_ticks: String,
    fields: Vec<ProcField>,
    tasks: NameCatalog,
    fds: FdCatalog,
}

impl Snapshot {
    fn capture(pid: u32, sequence: u64) -> Result<Self, CaptureFailure> {
        let started_unix_ns = unix_ns_or_zero();
        let started = Instant::now();
        let root = PathBuf::from(format!("/proc/{pid}"));
        let stat = fs::read(root.join("stat")).map_err(|error| {
            CaptureFailure::io(started_unix_ns, "proc-stat-unavailable-at-start", &error)
        })?;
        let identity = process::parse_stat(&stat).map_err(|detail| {
            CaptureFailure::invalid(started_unix_ns, "proc-stat-invalid-at-start", detail)
        })?;
        let reported_pid = identity.reported_pid.parse::<u32>().map_err(|_| {
            CaptureFailure::invalid(
                started_unix_ns,
                "proc-stat-invalid-at-start",
                "pid-is-not-u32",
            )
        })?;
        if reported_pid != pid {
            return Err(CaptureFailure::invalid(
                started_unix_ns,
                "proc-stat-pid-mismatch-at-start",
                format!("requested={pid} observed={reported_pid}"),
            ));
        }
        if matches!(identity.state, "Z" | "X" | "x") {
            return Err(CaptureFailure::terminal(
                started_unix_ns,
                "process-not-live-at-capture-start",
                identity.state.to_owned(),
                stat,
            ));
        }
        let initial_start_time_ticks = identity.start_time_ticks.to_owned();

        let mut fields = Vec::with_capacity(PROC_FILES.len() + 2);
        fields.push(ProcField {
            field: "last_live_proc_stat_begin_hex",
            evidence: ReadEvidence::Captured {
                bytes: stat,
                truncated: false,
            },
        });
        for (field, relative, limit) in PROC_FILES {
            let evidence = read_limited(&root.join(relative), *limit);
            fields.push(ProcField { field, evidence });
        }
        let tasks = NameCatalog::capture(&root.join("task"), MAX_TASK_CATALOG_BYTES);
        let fds = FdCatalog::capture(&root);
        if fields.iter().any(|field| {
            field.field == "last_live_proc_maps_hex"
                && matches!(
                    &field.evidence,
                    ReadEvidence::Captured { bytes, .. } if bytes.is_empty()
                )
        }) {
            return Err(CaptureFailure::invalid(
                started_unix_ns,
                "process-address-space-empty-during-capture",
                b"/proc/PID/maps became empty before the terminal wait event".to_vec(),
            ));
        }
        let final_stat = fs::read(root.join("stat")).map_err(|error| {
            CaptureFailure::io(started_unix_ns, "proc-stat-unavailable-at-end", &error)
        })?;
        let final_identity = process::parse_stat(&final_stat).map_err(|detail| {
            CaptureFailure::invalid(started_unix_ns, "proc-stat-invalid-at-end", detail)
        })?;
        let final_reported_pid = final_identity.reported_pid.parse::<u32>().map_err(|_| {
            CaptureFailure::invalid(
                started_unix_ns,
                "proc-stat-invalid-at-end",
                "pid-is-not-u32",
            )
        })?;
        if final_reported_pid != pid || final_identity.start_time_ticks != initial_start_time_ticks
        {
            return Err(CaptureFailure::invalid(
                started_unix_ns,
                "proc-identity-changed-during-capture",
                format!(
                    "requested_pid={pid} final_pid={final_reported_pid} initial_start_ticks={initial_start_time_ticks} final_start_ticks={}",
                    final_identity.start_time_ticks
                ),
            ));
        }
        if matches!(final_identity.state, "Z" | "X" | "x") {
            return Err(CaptureFailure::terminal(
                started_unix_ns,
                "process-became-terminal-during-capture",
                final_identity.state.to_owned(),
                final_stat,
            ));
        }
        let final_process_state = final_identity.state.to_owned();
        fields.push(ProcField {
            field: "last_live_proc_stat_end_hex",
            evidence: ReadEvidence::Captured {
                bytes: final_stat,
                truncated: false,
            },
        });
        let finished_unix_ns = unix_ns_or_zero();
        Ok(Self {
            pid,
            sequence,
            started_unix_ns,
            finished_unix_ns,
            capture_duration_ns: started.elapsed().as_nanos(),
            captured_at: Instant::now(),
            process_state: final_process_state,
            start_time_ticks: initial_start_time_ticks,
            fields,
            tasks,
            fds,
        })
    }

    fn summary_event_fields(&self, event: Event) -> Event {
        event
            .field("last_live_snapshot_state", "captured")
            .field("last_live_snapshot_pid", self.pid)
            .field("last_live_snapshot_sequence", self.sequence)
            .field("last_live_snapshot_started_unix_ns", self.started_unix_ns)
            .field("last_live_snapshot_finished_unix_ns", self.finished_unix_ns)
            .field(
                "last_live_snapshot_age_ns",
                self.captured_at.elapsed().as_nanos(),
            )
            .field(
                "last_live_snapshot_capture_duration_ns",
                self.capture_duration_ns,
            )
            .field("last_live_snapshot_process_state", &self.process_state)
            .field(
                "last_live_snapshot_start_time_ticks",
                &self.start_time_ticks,
            )
            .field("last_live_snapshot_fd_entries_seen", self.fds.entries_seen)
    }

    fn full_event_fields(&self, mut event: Event) -> Event {
        let mut captured = 0_usize;
        let mut failed = 0_usize;
        let mut truncated = Vec::new();
        for field in &self.fields {
            match &field.evidence {
                ReadEvidence::Captured {
                    bytes,
                    truncated: was_truncated,
                } => {
                    captured += 1;
                    if *was_truncated {
                        truncated.push(field.field);
                    }
                    event = event.field(field.field, hex_bytes(bytes));
                }
                ReadEvidence::Unavailable(error) => {
                    failed += 1;
                    event = event.field(field.field, error.render());
                }
            }
        }
        event = event
            .field("last_live_snapshot_payload_state", "captured")
            .field("last_live_proc_fields_captured", captured)
            .field("last_live_proc_fields_failed", failed)
            .field(
                "last_live_proc_truncated_fields",
                if truncated.is_empty() {
                    "NONE".to_owned()
                } else {
                    truncated.join(",")
                },
            );
        event = self.tasks.event_fields(event);
        self.fds.event_fields(event)
    }
}

#[derive(Debug)]
struct ProcField {
    field: &'static str,
    evidence: ReadEvidence,
}

#[derive(Debug)]
enum ReadEvidence {
    Captured { bytes: Vec<u8>, truncated: bool },
    Unavailable(ReadFailure),
}

#[derive(Debug)]
struct ReadFailure {
    kind: io::ErrorKind,
    raw_os_error: Option<i32>,
    message: Vec<u8>,
}

impl ReadFailure {
    fn from_error(error: &io::Error) -> Self {
        Self {
            kind: error.kind(),
            raw_os_error: error.raw_os_error(),
            message: error.to_string().into_bytes(),
        }
    }

    fn render(&self) -> String {
        format!(
            "UNAVAILABLE:{:?}:{}:{}",
            self.kind,
            optional_i32(self.raw_os_error),
            hex_bytes(&self.message)
        )
    }

    fn encoded(&self) -> String {
        hex_bytes(self.render().as_bytes())
    }
}

fn read_limited(path: &Path, limit: usize) -> ReadEvidence {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) => return ReadEvidence::Unavailable(ReadFailure::from_error(&error)),
    };
    let mut bytes = Vec::new();
    match file
        .take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)
    {
        Ok(_) => {
            let truncated = bytes.len() > limit;
            bytes.truncate(limit);
            ReadEvidence::Captured { bytes, truncated }
        }
        Err(error) => ReadEvidence::Unavailable(ReadFailure::from_error(&error)),
    }
}

#[derive(Debug)]
struct NameCatalog {
    state: &'static str,
    names: String,
    entries_seen: usize,
    entries_emitted: usize,
    entry_errors: usize,
    entry_errors_emitted: usize,
    entry_errors_truncated: usize,
    entry_error_catalog: String,
    truncated: usize,
    directory_error: Option<ReadFailure>,
}

impl NameCatalog {
    fn capture(path: &Path, budget: usize) -> Self {
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => {
                return Self {
                    state: "directory-unavailable",
                    names: String::new(),
                    entries_seen: 0,
                    entries_emitted: 0,
                    entry_errors: 0,
                    entry_errors_emitted: 0,
                    entry_errors_truncated: 0,
                    entry_error_catalog: String::new(),
                    truncated: 0,
                    directory_error: Some(ReadFailure::from_error(&error)),
                };
            }
        };
        let mut names = Vec::new();
        let mut entry_errors = 0_usize;
        let mut entry_errors_emitted = 0_usize;
        let mut entry_errors_truncated = 0_usize;
        let mut entry_error_catalog = String::new();
        for entry in entries {
            match entry {
                Ok(entry) => names.push(entry.file_name()),
                Err(error) => {
                    entry_errors += 1;
                    let encoded = ReadFailure::from_error(&error).encoded();
                    if push_bounded(&mut entry_error_catalog, &encoded, budget) {
                        entry_errors_emitted += 1;
                    } else {
                        entry_errors_truncated += 1;
                    }
                }
            }
        }
        names.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        let entries_seen = names.len();
        let mut rendered = String::new();
        let mut entries_emitted = 0_usize;
        let mut truncated = 0_usize;
        for name in names {
            let encoded = hex_bytes(name.as_bytes());
            if push_bounded(&mut rendered, &encoded, budget) {
                entries_emitted += 1;
            } else {
                truncated += 1;
            }
        }
        Self {
            state: if entry_errors == 0 && truncated == 0 {
                "captured"
            } else {
                "partial"
            },
            names: rendered,
            entries_seen,
            entries_emitted,
            entry_errors,
            entry_errors_emitted,
            entry_errors_truncated,
            entry_error_catalog,
            truncated,
            directory_error: None,
        }
    }

    fn event_fields(&self, event: Event) -> Event {
        event
            .field("last_live_task_catalog_state", self.state)
            .field("last_live_task_entries_seen", self.entries_seen)
            .field("last_live_task_entries_emitted", self.entries_emitted)
            .field("last_live_task_entry_errors", self.entry_errors)
            .field(
                "last_live_task_entry_errors_emitted",
                self.entry_errors_emitted,
            )
            .field(
                "last_live_task_entry_errors_truncated",
                self.entry_errors_truncated,
            )
            .field(
                "last_live_task_entry_error_hex_catalog",
                if self.entry_error_catalog.is_empty() {
                    "NONE"
                } else {
                    &self.entry_error_catalog
                },
            )
            .field("last_live_task_entries_truncated", self.truncated)
            .field(
                "last_live_task_directory_error",
                self.directory_error
                    .as_ref()
                    .map_or_else(|| "NONE".to_owned(), ReadFailure::render),
            )
            .field(
                "last_live_task_id_hex_catalog",
                if self.names.is_empty() {
                    "NONE"
                } else {
                    &self.names
                },
            )
    }
}

#[derive(Debug)]
struct FdCatalog {
    state: &'static str,
    entries_seen: usize,
    target_entries_emitted: usize,
    fdinfo_entries_emitted: usize,
    entry_errors: usize,
    entry_errors_emitted: usize,
    entry_errors_truncated: usize,
    entry_error_catalog: String,
    target_errors: usize,
    fdinfo_errors: usize,
    target_entries_truncated: usize,
    fdinfo_entries_truncated: usize,
    targets: String,
    fdinfo: String,
    directory_error: Option<ReadFailure>,
}

impl FdCatalog {
    fn capture(proc_root: &Path) -> Self {
        let fd_root = proc_root.join("fd");
        let entries = match fs::read_dir(&fd_root) {
            Ok(entries) => entries,
            Err(error) => return Self::unavailable(error),
        };
        let mut names = Vec::new();
        let mut entry_errors = 0_usize;
        let mut entry_errors_emitted = 0_usize;
        let mut entry_errors_truncated = 0_usize;
        let mut entry_error_catalog = String::new();
        for entry in entries {
            match entry {
                Ok(entry) => names.push(entry.file_name()),
                Err(error) => {
                    entry_errors += 1;
                    let encoded = ReadFailure::from_error(&error).encoded();
                    if push_bounded(&mut entry_error_catalog, &encoded, MAX_FD_CATALOG_BYTES) {
                        entry_errors_emitted += 1;
                    } else {
                        entry_errors_truncated += 1;
                    }
                }
            }
        }
        names.sort_by(fd_name_order);
        let entries_seen = names.len();
        let mut targets = String::new();
        let mut fdinfo = String::new();
        let mut target_entries_emitted = 0_usize;
        let mut fdinfo_entries_emitted = 0_usize;
        let mut target_errors = 0_usize;
        let mut fdinfo_errors = 0_usize;
        let mut target_entries_truncated = 0_usize;
        let mut fdinfo_entries_truncated = 0_usize;
        for name in names {
            let name_hex = hex_bytes(name.as_bytes());
            let target_value = match fs::read_link(fd_root.join(&name)) {
                Ok(target) => hex_bytes(target.as_os_str().as_bytes()),
                Err(error) => {
                    target_errors += 1;
                    format!("ERROR{}", ReadFailure::from_error(&error).encoded())
                }
            };
            if push_bounded(
                &mut targets,
                &format!("{name_hex}:{target_value}"),
                MAX_FD_CATALOG_BYTES,
            ) {
                target_entries_emitted += 1;
            } else {
                target_entries_truncated += 1;
            }

            let info_value = match read_limited(
                &proc_root.join("fdinfo").join(&name),
                MAX_SMALL_PROC_FILE_BYTES,
            ) {
                ReadEvidence::Captured { bytes, truncated } => {
                    let prefix = if truncated { "TRUNCATED" } else { "" };
                    format!("{prefix}{}", hex_bytes(&bytes))
                }
                ReadEvidence::Unavailable(error) => {
                    fdinfo_errors += 1;
                    format!("ERROR{}", error.encoded())
                }
            };
            if push_bounded(
                &mut fdinfo,
                &format!("{name_hex}:{info_value}"),
                MAX_FD_CATALOG_BYTES,
            ) {
                fdinfo_entries_emitted += 1;
            } else {
                fdinfo_entries_truncated += 1;
            }
        }
        let partial = entry_errors != 0
            || target_errors != 0
            || fdinfo_errors != 0
            || target_entries_truncated != 0
            || fdinfo_entries_truncated != 0;
        Self {
            state: if partial { "partial" } else { "captured" },
            entries_seen,
            target_entries_emitted,
            fdinfo_entries_emitted,
            entry_errors,
            entry_errors_emitted,
            entry_errors_truncated,
            entry_error_catalog,
            target_errors,
            fdinfo_errors,
            target_entries_truncated,
            fdinfo_entries_truncated,
            targets,
            fdinfo,
            directory_error: None,
        }
    }

    fn unavailable(error: io::Error) -> Self {
        Self {
            state: "directory-unavailable",
            entries_seen: 0,
            target_entries_emitted: 0,
            fdinfo_entries_emitted: 0,
            entry_errors: 0,
            entry_errors_emitted: 0,
            entry_errors_truncated: 0,
            entry_error_catalog: String::new(),
            target_errors: 0,
            fdinfo_errors: 0,
            target_entries_truncated: 0,
            fdinfo_entries_truncated: 0,
            targets: String::new(),
            fdinfo: String::new(),
            directory_error: Some(ReadFailure::from_error(&error)),
        }
    }

    fn event_fields(&self, event: Event) -> Event {
        event
            .field("last_live_fd_catalog_state", self.state)
            .field("last_live_fd_entries_seen", self.entries_seen)
            .field(
                "last_live_fd_target_entries_emitted",
                self.target_entries_emitted,
            )
            .field(
                "last_live_fdinfo_entries_emitted",
                self.fdinfo_entries_emitted,
            )
            .field("last_live_fd_entry_errors", self.entry_errors)
            .field(
                "last_live_fd_entry_errors_emitted",
                self.entry_errors_emitted,
            )
            .field(
                "last_live_fd_entry_errors_truncated",
                self.entry_errors_truncated,
            )
            .field(
                "last_live_fd_entry_error_hex_catalog",
                if self.entry_error_catalog.is_empty() {
                    "NONE"
                } else {
                    &self.entry_error_catalog
                },
            )
            .field("last_live_fd_target_errors", self.target_errors)
            .field("last_live_fdinfo_errors", self.fdinfo_errors)
            .field(
                "last_live_fd_target_entries_truncated",
                self.target_entries_truncated,
            )
            .field(
                "last_live_fdinfo_entries_truncated",
                self.fdinfo_entries_truncated,
            )
            .field(
                "last_live_fd_directory_error",
                self.directory_error
                    .as_ref()
                    .map_or_else(|| "NONE".to_owned(), ReadFailure::render),
            )
            .field(
                "last_live_fd_target_hex_catalog",
                if self.targets.is_empty() {
                    "NONE"
                } else {
                    &self.targets
                },
            )
            .field(
                "last_live_fdinfo_hex_catalog",
                if self.fdinfo.is_empty() {
                    "NONE"
                } else {
                    &self.fdinfo
                },
            )
    }
}

fn fd_name_order(left: &OsString, right: &OsString) -> std::cmp::Ordering {
    match (
        std::str::from_utf8(left.as_bytes())
            .ok()
            .and_then(|value| value.parse::<u64>().ok()),
        std::str::from_utf8(right.as_bytes())
            .ok()
            .and_then(|value| value.parse::<u64>().ok()),
    ) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => left.as_bytes().cmp(right.as_bytes()),
    }
}

fn push_bounded(destination: &mut String, value: &str, budget: usize) -> bool {
    let separator = usize::from(!destination.is_empty());
    if destination
        .len()
        .saturating_add(separator)
        .saturating_add(value.len())
        > budget
    {
        return false;
    }
    if separator != 0 {
        destination.push(',');
    }
    destination.push_str(value);
    true
}

fn unix_ns_or_zero() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

fn optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "NONE".to_owned(), |value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn retains_an_open_fd_from_a_real_live_process() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exec 9</dev/null; sleep 1"])
            .spawn()
            .expect("spawn live snapshot child");
        let mut tracker = Tracker::deferred(10);
        assert_eq!(tracker.capture_now(child.id()), None);
        let rendered = tracker
            .full_event_fields(Event::new("snapshot-test", 1, 2))
            .render();
        let _ = child.kill();
        let _ = child.wait();

        assert!(rendered.contains("\tlast_live_snapshot_state=captured\t"));
        assert!(rendered.contains("\tlast_live_snapshot_process_state="));
        assert!(rendered.contains("2f6465762f6e756c6c"), "{rendered}");
    }

    #[test]
    fn bounded_catalog_reports_omitted_entries() {
        let mut output = String::new();
        assert!(push_bounded(&mut output, "one", 7));
        assert!(push_bounded(&mut output, "two", 7));
        assert!(!push_bounded(&mut output, "three", 7));
        assert_eq!(output, "one,two");
    }

    #[test]
    fn reports_only_loss_and_restoration_transitions() {
        let mut tracker = Tracker::deferred(10);

        assert_eq!(
            tracker.capture_now(u32::MAX),
            Some(CaptureTransition::Unavailable)
        );
        assert_eq!(tracker.capture_now(u32::MAX), None);
        assert_eq!(
            tracker.capture_now(std::process::id()),
            Some(CaptureTransition::Restored)
        );
        assert_eq!(tracker.capture_now(std::process::id()), None);
    }
}
