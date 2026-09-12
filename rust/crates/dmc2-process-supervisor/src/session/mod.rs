mod failure;
mod output_capture;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::raw::{c_int, c_ulong};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant, SystemTime, SystemTimeError, UNIX_EPOCH};

use dmc2_diagnostics::RecoveryDisplay;

use crate::backtrace::{self, BacktraceEvidence};
use crate::catalog::{BacktraceKind, Ownership, ProcessRole};
use crate::cli::Invocation;
use crate::event::{encode_arguments, Event, EventTime};
use crate::journal::{FailureTracker, Journal};
use crate::limits::CoreDumpPlan;
use crate::process::{self, ProcessIdentity};
use crate::runtime::TRACKING_FAILURE_EXIT_CODE;
use crate::wait::{self, Poll, WaitEvidence};

pub use failure::SessionError;
use failure::{SessionObservationError, SessionObservationKind, StartupStage};
use output_capture::{PreparedCapture, RunningCapture};

const OBSERVATION_PERIOD: Duration = Duration::from_millis(10);

pub fn run_session(arguments: impl IntoIterator<Item = OsString>) -> Result<u8, SessionError> {
    let invocation = Invocation::parse(arguments).map_err(SessionError::Cli)?;
    if invocation.role.ownership() != Ownership::SessionRoot {
        return Err(SessionError::UnsupportedOwnership {
            role: invocation.role,
            ownership: invocation.role.ownership(),
        });
    }
    supervise(invocation)
}

fn supervise(invocation: Invocation) -> Result<u8, SessionError> {
    let supervisor_pid = std::process::id();
    let mut journal = Journal::open(&invocation.journal).map_err(SessionError::Journal)?;
    if let Err(source) = process::set_process_owner_identity(invocation.role) {
        return Err(startup_error(
            &mut journal,
            &invocation,
            supervisor_pid,
            StartupStage::OwnerIdentity,
            source,
        ));
    }
    let core_dump_plan = match CoreDumpPlan::capture(invocation.role.core_dump_policy()) {
        Ok(plan) => plan,
        Err(source) => {
            return Err(startup_error(
                &mut journal,
                &invocation,
                supervisor_pid,
                StartupStage::CoreDumpLimit,
                source,
            ))
        }
    };
    if let Err(source) = enable_child_subreaper() {
        return Err(startup_error(
            &mut journal,
            &invocation,
            supervisor_pid,
            StartupStage::EnableSubreaper,
            source,
        ));
    }
    if let Err(source) = verify_child_subreaper() {
        return Err(startup_error(
            &mut journal,
            &invocation,
            supervisor_pid,
            StartupStage::VerifySubreaper,
            source,
        ));
    }

    let session_started_ns = unix_ns().map_err(SessionError::Clock)?;
    let event = session_event_at(
        "session-supervisor-started",
        session_started_ns,
        supervisor_pid,
    )
    .field("role", invocation.role.name())
    .field("ownership", invocation.role.ownership().name())
    .field("owner_comm", invocation.role.owner_comm())
    .field("observation_period_ns", OBSERVATION_PERIOD.as_nanos())
    .field(
        "recovered_partial_record",
        journal.recovered_partial_record(),
    )
    .encoded_path_field("journal_path_hex", journal.path())
    .encoded_os_field("program_hex", &invocation.program)
    .field("argc", invocation.arguments.len())
    .field("argv_hex", encode_arguments(&invocation.arguments));
    let event = core_dump_plan.event_fields(event);
    let event = process::executable_event_fields(event, Path::new(&invocation.program));
    journal.append(&event).map_err(SessionError::Journal)?;

    let mut command = Command::new(&invocation.program);
    command.args(&invocation.arguments);
    core_dump_plan.configure(&mut command);
    let prepared_capture = match invocation.failure_report.as_deref() {
        Some(path) => Some(
            PreparedCapture::prepare(path, journal.path(), invocation.role.name(), &mut command)
                .map_err(|source| {
                    startup_error(
                        &mut journal,
                        &invocation,
                        supervisor_pid,
                        StartupStage::PrepareOutputCapture,
                        source,
                    )
                })?,
        ),
        None => None,
    };
    let mut linuxcnc = match command.spawn() {
        Ok(child) => child,
        Err(source) => {
            return Err(startup_error(
                &mut journal,
                &invocation,
                supervisor_pid,
                StartupStage::Spawn,
                source,
            ))
        }
    };
    let linuxcnc_pid = linuxcnc.id();
    let mut journal_failures = FailureTracker::new();
    let mut capture_failures = 0_u64;
    let mut capture_error = None;
    let mut running_capture = match prepared_capture {
        Some(capture) => match capture.start(&mut linuxcnc) {
            Ok(capture) => Some(capture),
            Err(source) => {
                capture_failures = 1;
                let observation = SessionObservationError::OutputCaptureStart {
                    linuxcnc_pid,
                    source: &source,
                };
                let event = session_event("session-output-capture-start-failed", supervisor_pid)
                    .field("linuxcnc_pid", linuxcnc_pid)
                    .field("error", observation.to_string())
                    .recovery(&observation);
                append_after_spawn(&mut journal, &event, &mut journal_failures);
                eprintln!("dmc2-session-supervisor: {}", RecoveryDisplay(&observation));
                capture_error = Some(source);
                None
            }
        },
        None => None,
    };

    let mut children = BTreeMap::new();
    let root = ObservedChild::new(ProcessIdentity::for_role(invocation.role, "launcher-role"));
    let event = root.started_event(supervisor_pid, linuxcnc_pid, "direct-session-child");
    append_after_spawn(&mut journal, &event, &mut journal_failures);
    children.insert(linuxcnc_pid, root);

    let mut linuxcnc_status = None;
    let mut child_scan_issue = IssueState::new(SessionObservationKind::ChildScan);
    let mut wait_issue = IssueState::new(SessionObservationKind::Wait4);
    loop {
        match direct_children(supervisor_pid) {
            Ok(pids) => {
                record_issue_recovery(
                    &mut child_scan_issue,
                    &mut journal,
                    supervisor_pid,
                    &mut journal_failures,
                );
                refresh_children(
                    pids,
                    &mut children,
                    &mut journal,
                    supervisor_pid,
                    &mut journal_failures,
                );
            }
            Err(error) => record_issue_failure(
                &mut child_scan_issue,
                error,
                &mut journal,
                supervisor_pid,
                &mut journal_failures,
            ),
        }

        let mut no_children = false;
        loop {
            match wait::poll_any() {
                Ok(Poll::Terminal(evidence)) => {
                    record_issue_recovery(
                        &mut wait_issue,
                        &mut journal,
                        supervisor_pid,
                        &mut journal_failures,
                    );
                    record_termination(
                        evidence,
                        linuxcnc_pid,
                        &mut linuxcnc_status,
                        &mut children,
                        &mut journal,
                        supervisor_pid,
                        &mut journal_failures,
                    );
                }
                Ok(Poll::Running) => {
                    record_issue_recovery(
                        &mut wait_issue,
                        &mut journal,
                        supervisor_pid,
                        &mut journal_failures,
                    );
                    break;
                }
                Ok(Poll::NoChildren) => {
                    record_issue_recovery(
                        &mut wait_issue,
                        &mut journal,
                        supervisor_pid,
                        &mut journal_failures,
                    );
                    no_children = true;
                    break;
                }
                Err(error) => {
                    record_issue_failure(
                        &mut wait_issue,
                        error,
                        &mut journal,
                        supervisor_pid,
                        &mut journal_failures,
                    );
                    break;
                }
            }
        }

        if no_children {
            let status =
                linuxcnc_status.ok_or(SessionError::LinuxCncStatusMissing { linuxcnc_pid })?;
            let failure_report = finish_capture(
                running_capture.take(),
                status,
                &invocation,
                session_started_ns,
                &mut capture_error,
                &mut capture_failures,
            );
            let event = session_event("session-supervisor-terminated", supervisor_pid)
                .field("linuxcnc_pid", linuxcnc_pid)
                .field("linuxcnc_raw_wait_status", status.into_raw())
                .field("linuxcnc_exit_code", optional_i32(status.code()))
                .field("linuxcnc_signal", optional_i32(status.signal()))
                .field("failure_report_written", failure_report.is_some())
                .field("remaining_tracked_children", children.len())
                .field("child_scan_failures", child_scan_issue.failures)
                .field("child_scan_recoveries", child_scan_issue.recoveries)
                .field("wait_failures", wait_issue.failures)
                .field("wait_recoveries", wait_issue.recoveries)
                .field("output_capture_failures", capture_failures);
            let event = match failure_report {
                Some(path) => event.encoded_path_field("failure_report_path_hex", &path),
                None => event.field("failure_report_path_hex", "NONE"),
            };
            let event = journal_failures.event_fields(event);
            append_after_spawn(&mut journal, &event, &mut journal_failures);
            if let Some(source) = capture_error {
                return Err(SessionError::OutputCaptureAfterSpawn {
                    report_path: invocation.failure_report.clone(),
                    source,
                    additional_failures: capture_failures.saturating_sub(1),
                    journal_failures: journal_failures.failures(),
                });
            }
            if let Some(first) = journal_failures.take_first() {
                return Err(SessionError::JournalAfterSpawn {
                    first,
                    additional_failures: journal_failures.additional_failures(),
                });
            }
            return Ok(supervisor_exit_code(status));
        }
        thread::sleep(OBSERVATION_PERIOD);
    }
}

fn record_termination(
    evidence: WaitEvidence,
    linuxcnc_pid: u32,
    linuxcnc_status: &mut Option<ExitStatus>,
    children: &mut BTreeMap<u32, ObservedChild>,
    journal: &mut Journal,
    supervisor_pid: u32,
    journal_failures: &mut FailureTracker,
) {
    let Some(child) = children.remove(&evidence.pid) else {
        let identity = ProcessIdentity::unknown(
            "unobserved-terminal-child",
            "terminal-status-only",
        );
        let event = identity.event_fields(
            session_event("session-child-terminated", supervisor_pid)
                .field("linuxcnc_pid", linuxcnc_pid)
                .field("is_linuxcnc_root", evidence.pid == linuxcnc_pid)
                .field("elapsed_since_observed_ns", "UNAVAILABLE")
                .field("observation_state", "identity-and-start-time-unavailable")
                .field("observation_error", "terminal status arrived before this child was observed; retain the raw exit evidence and recheck lifecycle tracking on the next UI launch"),
        );
        append_after_spawn(journal, &evidence.event_fields(event), journal_failures);
        if evidence.pid == linuxcnc_pid {
            *linuxcnc_status = Some(evidence.status);
        }
        return;
    };
    let backtrace = match child.identity.role().map(ProcessRole::backtrace) {
        Some(BacktraceKind::LinuxCncTask) => backtrace::capture(
            journal.path(),
            evidence.pid,
            child.first_observed_wall,
            unix_ns(),
        ),
        _ => BacktraceEvidence::NotApplicable,
    };
    let event = child.identity.event_fields(
        session_event("session-child-terminated", supervisor_pid)
            .field("linuxcnc_pid", linuxcnc_pid)
            .field("is_linuxcnc_root", evidence.pid == linuxcnc_pid)
            .field(
                "elapsed_since_observed_ns",
                child.first_observed.elapsed().as_nanos(),
            ),
    );
    let event = evidence.event_fields(event);
    let event = backtrace.event_fields(event);
    append_after_spawn(journal, &event, journal_failures);
    if evidence.pid == linuxcnc_pid {
        *linuxcnc_status = Some(evidence.status);
    }
}

fn refresh_children(
    pids: Vec<u32>,
    children: &mut BTreeMap<u32, ObservedChild>,
    journal: &mut Journal,
    supervisor_pid: u32,
    failures: &mut FailureTracker,
) {
    for pid in pids {
        let observed = process::identify(pid);
        if let Some(existing) = children.get_mut(&pid) {
            if observed.is_more_specific_than(&existing.identity) {
                let previous = std::mem::replace(&mut existing.identity, observed);
                let event = existing.identity.event_fields(
                    session_event("session-child-reclassified", supervisor_pid)
                        .field("child_pid", pid)
                        .field("previous_identity", format!("{previous:?}")),
                );
                append_after_spawn(journal, &event, failures);
            }
            continue;
        }
        let child = ObservedChild::new(observed);
        let event = child.started_event(supervisor_pid, pid, "adopted-session-descendant");
        append_after_spawn(journal, &event, failures);
        children.insert(pid, child);
    }
}

fn finish_capture(
    capture: Option<RunningCapture>,
    status: ExitStatus,
    invocation: &Invocation,
    session_started_ns: u128,
    first_error: &mut Option<io::Error>,
    failure_count: &mut u64,
) -> Option<std::path::PathBuf> {
    let Some(capture) = capture else {
        return None;
    };
    match capture.finish(
        status,
        &invocation.program,
        &invocation.arguments,
        session_started_ns,
    ) {
        Ok(path) => path,
        Err(error) => {
            *failure_count = (*failure_count).saturating_add(1);
            let observation = SessionObservationError::OutputCaptureFinish { source: &error };
            eprintln!("dmc2-session-supervisor: {}", RecoveryDisplay(&observation));
            if first_error.is_none() {
                *first_error = Some(error);
            }
            None
        }
    }
}

fn startup_error(
    journal: &mut Journal,
    invocation: &Invocation,
    supervisor_pid: u32,
    stage: StartupStage,
    source: io::Error,
) -> SessionError {
    let event = session_event("session-startup-failed", supervisor_pid)
        .field("role", invocation.role.name())
        .field("stage", format!("{stage:?}"))
        .field("error", source.to_string());
    let journal_error = journal.append(&event).err();
    SessionError::startup(
        stage,
        invocation.role,
        invocation.program.clone(),
        source,
        journal_error,
    )
}

fn append_after_spawn(journal: &mut Journal, event: &Event, failures: &mut FailureTracker) {
    match journal.append(event) {
        Ok(()) => {
            if failures.record_success() {
                eprintln!("dmc2-session-supervisor: lifecycle journal recovered");
            }
        }
        Err(error) => {
            eprintln!(
                "dmc2-session-supervisor: lifecycle journal failed: {}; writing will be retried while the LinuxCNC session remains owned",
                RecoveryDisplay(&error),
            );
            failures.record_failure(error);
        }
    }
}

struct ObservedChild {
    identity: ProcessIdentity,
    first_observed: Instant,
    first_observed_wall: SystemTime,
}

impl ObservedChild {
    fn new(identity: ProcessIdentity) -> Self {
        Self {
            identity,
            first_observed: Instant::now(),
            first_observed_wall: SystemTime::now(),
        }
    }

    fn started_event(&self, supervisor_pid: u32, pid: u32, relation: &'static str) -> Event {
        self.identity.event_fields(process::event_fields(
            session_event("session-child-observed", supervisor_pid)
                .field("relation", relation)
                .field(
                    "first_observed_unix_ns",
                    system_time_unix_ns(self.first_observed_wall),
                ),
            pid,
        ))
    }
}

struct IssueState {
    kind: SessionObservationKind,
    active: bool,
    failures: u64,
    recoveries: u64,
}

impl IssueState {
    const fn new(kind: SessionObservationKind) -> Self {
        Self {
            kind,
            active: false,
            failures: 0,
            recoveries: 0,
        }
    }
    fn fail(&mut self) -> bool {
        self.failures = self.failures.saturating_add(1);
        let transition = !self.active;
        self.active = true;
        transition
    }
    fn recover(&mut self) -> bool {
        if self.active {
            self.active = false;
            self.recoveries = self.recoveries.saturating_add(1);
            true
        } else {
            false
        }
    }
}

fn record_issue_failure(
    issue: &mut IssueState,
    error: io::Error,
    journal: &mut Journal,
    supervisor_pid: u32,
    failures: &mut FailureTracker,
) {
    if issue.fail() {
        let observation = SessionObservationError::Source {
            kind: issue.kind,
            source: &error,
        };
        let event = session_event("session-observation-failed", supervisor_pid)
            .field("state", issue.kind.name())
            .field("error", observation.to_string())
            .recovery(&observation);
        append_after_spawn(journal, &event, failures);
        eprintln!("dmc2-session-supervisor: {}", RecoveryDisplay(&observation));
    }
}

fn record_issue_recovery(
    issue: &mut IssueState,
    journal: &mut Journal,
    supervisor_pid: u32,
    failures: &mut FailureTracker,
) {
    if issue.recover() {
        let event = session_event("session-observation-recovered", supervisor_pid)
            .field("state", issue.kind.name())
            .field("failures", issue.failures)
            .field("recoveries", issue.recoveries);
        append_after_spawn(journal, &event, failures);
        eprintln!(
            "dmc2-session-supervisor: state={} transition={} recovered",
            issue.kind.name(),
            dmc2_diagnostics::RecoveryClass::RecheckSource
                .transition()
                .name(),
        );
    }
}

fn direct_children(supervisor_pid: u32) -> io::Result<Vec<u32>> {
    let contents = fs::read_to_string(format!("/proc/self/task/{supervisor_pid}/children"))?;
    contents
        .split_ascii_whitespace()
        .map(|field| {
            field.parse::<u32>().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid child PID {field:?}: {error}"),
                )
            })
        })
        .collect()
}

fn enable_child_subreaper() -> io::Result<()> {
    const PR_SET_CHILD_SUBREAPER: c_int = 36;
    // SAFETY: this prctl consumes an integer flag and retains no pointer.
    if unsafe { prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn verify_child_subreaper() -> io::Result<()> {
    const PR_GET_CHILD_SUBREAPER: c_int = 37;
    let mut enabled = 0_i32;
    // SAFETY: the kernel writes one c_int through this live local pointer.
    let result = unsafe {
        prctl(
            PR_GET_CHILD_SUBREAPER,
            (&mut enabled as *mut c_int) as c_ulong,
            0,
            0,
            0,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if enabled == 1 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "PR_GET_CHILD_SUBREAPER returned {enabled}"
        )))
    }
}

fn session_event(kind: &'static str, supervisor_pid: u32) -> Event {
    session_event_at(kind, unix_ns(), supervisor_pid)
}
fn session_event_at(kind: &'static str, unix_ns: impl Into<EventTime>, supervisor_pid: u32) -> Event {
    Event::new(kind, unix_ns, supervisor_pid).field("tracker", "session-subreaper")
}

fn supervisor_exit_code(status: ExitStatus) -> u8 {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .or_else(|| {
            status
                .signal()
                .and_then(|signal| u8::try_from(128_i32.saturating_add(signal)).ok())
        })
        .unwrap_or(TRACKING_FAILURE_EXIT_CODE)
}

fn optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "NONE".to_owned(), |value| value.to_string())
}
fn unix_ns() -> Result<u128, SystemTimeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
}
fn system_time_unix_ns(value: SystemTime) -> EventTime {
    value
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .into()
}

unsafe extern "C" {
    fn prctl(
        option: c_int,
        argument2: c_ulong,
        argument3: c_ulong,
        argument4: c_ulong,
        argument5: c_ulong,
    ) -> c_int;
}
