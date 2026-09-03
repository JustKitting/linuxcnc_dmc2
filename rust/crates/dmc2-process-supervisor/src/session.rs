use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::raw::{c_int, c_ulong};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant, SystemTime, SystemTimeError, UNIX_EPOCH};

use crate::backtrace::BacktraceEvidence;
use crate::catalog::{self, Ownership, ProcessRole};
use crate::cli::Invocation;
use crate::core_artifact::{self, WorkingDirectoryEvidence};
use crate::event::{encode_arguments, hex_bytes, Event};
use crate::journal::{FailureTracker, Journal};
use crate::limits::CoreDumpPlan;
use crate::live_snapshot;
use crate::process;
use crate::runtime::TRACKING_FAILURE_EXIT_CODE;
use crate::wait::{self, AnyReap, AnyWait, TerminalObservation, WaitEvidence};
use crate::wait_degradation::WaitDegradation;

mod error;
mod reap;
mod terminal;

pub use error::SessionError;
use reap::reap_retained_session_child;
use terminal::{
    child_terminal_observed_event, child_terminal_reaped_fallback_event, child_terminated_event,
    session_backtrace, SessionRootObservation,
};

const OBSERVATION_PERIOD: Duration = Duration::from_millis(5);

enum SessionTerminalAcquisition {
    Retained(TerminalObservation),
    ReapedFallback {
        evidence: WaitEvidence,
        degradation: WaitDegradation,
    },
}

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
        let event = session_event(
            "session-owner-identity-failed",
            unix_ns_or_zero(),
            supervisor_pid,
        )
        .field("role", invocation.role.name())
        .field("owner_comm", invocation.role.owner_comm())
        .field("error_kind", format!("{:?}", source.kind()))
        .field("raw_os_error", optional_i32(source.raw_os_error()))
        .field("error_hex", hex_bytes(source.to_string().as_bytes()));
        return match journal.append(&event) {
            Ok(()) => Err(SessionError::OwnerIdentity {
                role: invocation.role,
                source,
            }),
            Err(journal) => Err(SessionError::OwnerIdentityAndJournal {
                role: invocation.role,
                identity: source,
                journal,
            }),
        };
    }
    let core_dump_plan = match CoreDumpPlan::capture(invocation.role.core_dump_policy()) {
        Ok(plan) => plan,
        Err(source) => {
            let event = session_event(
                "session-core-limit-plan-failed",
                unix_ns_or_zero(),
                supervisor_pid,
            )
            .field("role", invocation.role.name())
            .field("error_kind", format!("{:?}", source.kind()))
            .field("raw_os_error", optional_i32(source.raw_os_error()))
            .field("error_hex", hex_bytes(source.to_string().as_bytes()));
            return match journal.append(&event) {
                Ok(()) => Err(SessionError::CoreDumpLimit {
                    role: invocation.role,
                    source,
                }),
                Err(journal) => Err(SessionError::CoreDumpLimitAndJournal {
                    role: invocation.role,
                    limit: source,
                    journal,
                }),
            };
        }
    };
    enable_child_subreaper().map_err(SessionError::EnableSubreaper)?;
    verify_child_subreaper().map_err(SessionError::VerifySubreaper)?;

    let session_started_ns = unix_ns().map_err(SessionError::Clock)?;
    let fallback_cwd = WorkingDirectoryEvidence::for_supervisor();
    let event = session_event(
        "session-supervisor-started",
        session_started_ns,
        supervisor_pid,
    )
    .field("role", invocation.role.name())
    .field("ownership", invocation.role.ownership().name())
    .field("owner_comm", invocation.role.owner_comm())
    .field(
        "catalog_live_snapshot_period_ms",
        invocation.role.live_snapshot_period_ms(),
    )
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
    let event = process::environment_event_fields(event);
    let event = process::host_event_fields(event);
    let event = process::executable_event_fields(event, Path::new(&invocation.program));
    let event = process::child_event_fields(event, supervisor_pid);
    journal.append(&event).map_err(SessionError::Journal)?;

    let mut command = Command::new(&invocation.program);
    command.args(&invocation.arguments);
    core_dump_plan.configure(&mut command);
    let linuxcnc = match command.spawn() {
        Ok(child) => child,
        Err(source) => {
            let event = session_event("session-spawn-failed", unix_ns_or_zero(), supervisor_pid)
                .field("role", invocation.role.name())
                .encoded_os_field("program_hex", &invocation.program)
                .field("error_kind", format!("{:?}", source.kind()))
                .field("raw_os_error", optional_i32(source.raw_os_error()))
                .field("error_hex", hex_bytes(source.to_string().as_bytes()));
            return match journal.append(&event) {
                Ok(()) => Err(SessionError::Spawn {
                    role: invocation.role,
                    program: invocation.program,
                    source,
                }),
                Err(journal) => Err(SessionError::SpawnAndJournal {
                    role: invocation.role,
                    program: invocation.program,
                    spawn: source,
                    journal,
                }),
            };
        }
    };
    let linuxcnc_pid = linuxcnc.id();
    let observed_at = Instant::now();
    let observed_at_wall = SystemTime::now();
    let mut children = BTreeMap::new();
    let mut root = ObservedChild {
        role: invocation.role.name().to_owned(),
        catalog_role: Some(invocation.role),
        relation: "direct-session-child",
        layer: "session-root",
        identity_source: "launcher-role",
        first_observed: observed_at,
        first_observed_wall: observed_at_wall,
        working_directory: WorkingDirectoryEvidence::for_process(linuxcnc_pid),
        live_snapshots: live_snapshot::Tracker::deferred(invocation.role.live_snapshot_period_ms()),
    };
    let root_snapshot_transition = root.live_snapshots.capture_now(linuxcnc_pid);
    let event = child_started_event(supervisor_pid, linuxcnc_pid, &root);
    let mut retained_journal_errors = FailureTracker::new();
    append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
    if let Some(transition) = root_snapshot_transition {
        append_child_live_snapshot_transition(
            &mut journal,
            supervisor_pid,
            linuxcnc_pid,
            &root,
            transition,
            &mut retained_journal_errors,
        );
    }
    children.insert(linuxcnc_pid, root);

    let mut linuxcnc_status = None;
    let mut children_probe_error_reported = false;
    let mut wait_degradation: Option<WaitDegradation> = None;
    let mut reap_degradations = Vec::new();
    loop {
        refresh_session_children(
            &mut journal,
            supervisor_pid,
            invocation.role.live_snapshot_period_ms(),
            &mut children,
            &mut children_probe_error_reported,
            &mut retained_journal_errors,
        );

        for (pid, child) in &mut children {
            if let Some(transition) = child.live_snapshots.capture_if_due(*pid) {
                append_child_live_snapshot_transition(
                    &mut journal,
                    supervisor_pid,
                    *pid,
                    child,
                    transition,
                    &mut retained_journal_errors,
                );
            }
        }

        let mut no_children = false;
        loop {
            let acquisition = if let Some(degradation) = &mut wait_degradation {
                match wait::reap_any_nonblocking() {
                    Ok(AnyReap::Terminal(evidence)) => {
                        if degradation.record_fallback_success() {
                            let event = degradation.summary_event_fields(
                                session_event(
                                    "session-fallback-reap-poll-restored",
                                    unix_ns_or_zero(),
                                    supervisor_pid,
                                )
                                .field("fallback_wait4_result", "terminal-reaped"),
                            );
                            append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                        }
                        SessionTerminalAcquisition::ReapedFallback {
                            evidence,
                            degradation: degradation.clone(),
                        }
                    }
                    Ok(AnyReap::Running) => {
                        if degradation.record_fallback_success() {
                            let event = degradation.summary_event_fields(
                                session_event(
                                    "session-fallback-reap-poll-restored",
                                    unix_ns_or_zero(),
                                    supervisor_pid,
                                )
                                .field("fallback_wait4_result", "children-running"),
                            );
                            append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                        }
                        break;
                    }
                    Ok(AnyReap::NoChildren) => {
                        if degradation.record_fallback_success() {
                            let event = degradation.summary_event_fields(
                                session_event(
                                    "session-fallback-reap-poll-restored",
                                    unix_ns_or_zero(),
                                    supervisor_pid,
                                )
                                .field("fallback_wait4_result", "no-children"),
                            );
                            append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                        }
                        no_children = true;
                        break;
                    }
                    Err(error) => {
                        if let Some(error) = degradation.record_fallback_failure(&error) {
                            let event = error.fallback_event_fields(
                                degradation.summary_event_fields(
                                    session_event(
                                        "session-fallback-reap-poll-failed",
                                        unix_ns_or_zero(),
                                        supervisor_pid,
                                    )
                                    .field("session_ownership_released", false),
                                ),
                            );
                            append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                        }
                        break;
                    }
                }
            } else {
                match wait::observe_any_nonblocking() {
                    Ok(AnyWait::Terminal(observation)) => {
                        SessionTerminalAcquisition::Retained(observation)
                    }
                    Ok(AnyWait::Running) => break,
                    Ok(AnyWait::NoChildren) => {
                        no_children = true;
                        break;
                    }
                    Err(error) => {
                        let degradation = WaitDegradation::new(error);
                        let event = degradation.waitid_event_fields(
                            session_event("session-wait-failed", unix_ns_or_zero(), supervisor_pid)
                                .field("session_ownership_released", false)
                                .field(
                                    "terminal_acquisition_fallback",
                                    "wait4-p-all-wnohang-reaping",
                                ),
                        );
                        append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                        wait_degradation = Some(degradation);
                        continue;
                    }
                }
            };

            match acquisition {
                SessionTerminalAcquisition::Retained(observation) => {
                    let terminal_identity =
                        classify_child(observation.pid, invocation.role.live_snapshot_period_ms());
                    if let Some(existing) = children.get_mut(&observation.pid) {
                        if existing.should_reclassify_as(&terminal_identity) {
                            let event = child_reclassified_event(
                                supervisor_pid,
                                observation.pid,
                                existing,
                                &terminal_identity,
                            );
                            append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                            existing.reclassify_from(&terminal_identity);
                        }
                    }
                    let session_root = SessionRootObservation::capture(
                        observation.pid,
                        linuxcnc_pid,
                        linuxcnc_status.is_some(),
                    );
                    let observation_state = if children.contains_key(&observation.pid) {
                        "start-observed"
                    } else {
                        "terminal-only"
                    };
                    let child = children.get(&observation.pid).unwrap_or(&terminal_identity);
                    let terminal_event = child_terminal_observed_event(
                        supervisor_pid,
                        linuxcnc_pid,
                        &session_root,
                        Some(child),
                        &terminal_identity,
                        observation_state,
                        observation,
                        &children,
                    );
                    append_after_spawn(&mut journal, &terminal_event, &mut retained_journal_errors);
                    let exit_ns = unix_ns_or_zero();
                    let backtrace = session_backtrace(
                        journal.path(),
                        child,
                        observation.pid,
                        child.first_observed_wall,
                        exit_ns,
                    );
                    let core_artifact = core_artifact::capture(
                        journal.path(),
                        observation.pid,
                        observation.core_dumped(),
                        &child.working_directory,
                        &fallback_cwd,
                        child.first_observed_wall,
                        exit_ns,
                    );
                    let (evidence, reap_degradation) = reap_retained_session_child(
                        &mut journal,
                        supervisor_pid,
                        observation.pid,
                        observation,
                        invocation.role.live_snapshot_period_ms(),
                        &mut children,
                        &mut children_probe_error_reported,
                        &mut retained_journal_errors,
                    );
                    let child = children.remove(&evidence.pid).unwrap_or(terminal_identity);
                    let event = child_terminated_event(
                        supervisor_pid,
                        linuxcnc_pid,
                        &session_root,
                        Some(&child),
                        observation_state,
                        exit_ns,
                        Some(observation),
                        evidence,
                        &backtrace,
                        &core_artifact,
                    );
                    let event = match &reap_degradation {
                        Some(degradation) => degradation.summary_event_fields(event),
                        None => event.field("terminal_reap_degraded", false),
                    };
                    append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                    if let Some(degradation) = reap_degradation {
                        reap_degradations.push((evidence.pid, degradation));
                    }
                    if evidence.pid == linuxcnc_pid {
                        linuxcnc_status = Some(evidence.status);
                    }
                }
                SessionTerminalAcquisition::ReapedFallback {
                    evidence,
                    degradation,
                } => {
                    let child_pid = evidence.pid;
                    let session_root = SessionRootObservation::capture(
                        child_pid,
                        linuxcnc_pid,
                        linuxcnc_status.is_some(),
                    );
                    let observation_state = if children.contains_key(&child_pid) {
                        "start-observed-reaped-fallback"
                    } else {
                        "terminal-only-reaped-fallback"
                    };
                    let terminal_event = child_terminal_reaped_fallback_event(
                        supervisor_pid,
                        linuxcnc_pid,
                        &session_root,
                        children.get(&child_pid),
                        observation_state,
                        evidence,
                        &children,
                        &degradation,
                    );
                    append_after_spawn(&mut journal, &terminal_event, &mut retained_journal_errors);
                    let exit_ns = unix_ns_or_zero();
                    let (backtrace, core_artifact) = match children.get(&child_pid) {
                        Some(child) => (
                            session_backtrace(
                                journal.path(),
                                child,
                                child_pid,
                                child.first_observed_wall,
                                exit_ns,
                            ),
                            core_artifact::capture(
                                journal.path(),
                                child_pid,
                                evidence.status.core_dumped(),
                                &child.working_directory,
                                &fallback_cwd,
                                child.first_observed_wall,
                                exit_ns,
                            ),
                        ),
                        None => (
                            BacktraceEvidence::NotApplicable,
                            core_artifact::capture(
                                journal.path(),
                                child_pid,
                                evidence.status.core_dumped(),
                                &fallback_cwd,
                                &fallback_cwd,
                                observed_at_wall,
                                exit_ns,
                            ),
                        ),
                    };
                    let child = children.remove(&child_pid);
                    let event = child_terminated_event(
                        supervisor_pid,
                        linuxcnc_pid,
                        &session_root,
                        child.as_ref(),
                        observation_state,
                        exit_ns,
                        None,
                        evidence,
                        &backtrace,
                        &core_artifact,
                    );
                    let event = degradation
                        .summary_event_fields(event)
                        .field("terminal_reap_degraded", false);
                    append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                    if child_pid == linuxcnc_pid {
                        linuxcnc_status = Some(evidence.status);
                    }
                }
            }
        }

        if no_children {
            let Some(status) = linuxcnc_status else {
                let event = session_event(
                    "session-invariant-failed",
                    unix_ns_or_zero(),
                    supervisor_pid,
                )
                .field("identity", "LINUXCNC_ROOT_WAIT_STATUS_MISSING")
                .field("linuxcnc_pid", linuxcnc_pid)
                .field("remaining_children", children.len());
                append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                return Err(SessionError::LinuxCncStatusMissing { linuxcnc_pid });
            };
            let event = session_event(
                "session-supervisor-terminated",
                unix_ns_or_zero(),
                supervisor_pid,
            )
            .field("linuxcnc_pid", linuxcnc_pid)
            .field("linuxcnc_raw_wait_status", status.into_raw())
            .field("linuxcnc_exit_code", optional_i32(status.code()))
            .field("linuxcnc_signal", optional_i32(status.signal()))
            .field("remaining_children", children.len());
            let event = match &wait_degradation {
                Some(degradation) => degradation.summary_event_fields(event),
                None => event.field("terminal_observation_degraded", false),
            };
            let reap_wait4_failures = reap_degradations
                .iter()
                .map(|(_, degradation)| degradation.failures())
                .fold(0_u64, u64::saturating_add);
            let event = event
                .field("terminal_reap_degraded", !reap_degradations.is_empty())
                .field("terminal_reap_degraded_children", reap_degradations.len())
                .field("terminal_reap_wait4_failures", reap_wait4_failures)
                .field(
                    "terminal_reap_degraded_pid_list",
                    reap_degradations
                        .iter()
                        .map(|(pid, _)| pid.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                );
            let event = retained_journal_errors.summary_event_fields(event);
            append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
            if let Some(degradation) = wait_degradation.take() {
                let first_journal_failure = retained_journal_errors.take_first();
                return Err(SessionError::TerminalObservationDegraded {
                    waitid_error_kind: degradation.waitid_error().kind,
                    waitid_raw_os_error: degradation.waitid_error().raw_os_error,
                    waitid_error: degradation.waitid_error().detail.clone(),
                    fallback_wait4_failures: degradation.fallback_wait4_failures(),
                    retained_reap_degraded_children: reap_degradations.len(),
                    retained_reap_wait4_failures: reap_wait4_failures,
                    first_journal_failure,
                    additional_journal_failures: retained_journal_errors.additional_failures(),
                });
            }
            if !reap_degradations.is_empty() {
                let (child_pid, degradation) = reap_degradations.remove(0);
                let first_journal_failure = retained_journal_errors.take_first();
                return Err(SessionError::TerminalReapDegraded {
                    child_pid,
                    additional_affected_children: reap_degradations.len(),
                    first_error_kind: degradation.first_error().kind,
                    first_raw_os_error: degradation.first_error().raw_os_error,
                    first_error: degradation.first_error().detail.clone(),
                    wait4_failures: reap_wait4_failures,
                    first_journal_failure,
                    additional_journal_failures: retained_journal_errors.additional_failures(),
                });
            }
            if let Some(first) = retained_journal_errors.take_first() {
                return Err(SessionError::JournalAfterSpawn {
                    first,
                    additional_failures: retained_journal_errors.additional_failures(),
                });
            }
            return Ok(supervisor_exit_code(status));
        }

        thread::sleep(OBSERVATION_PERIOD);
    }
}

fn refresh_session_children(
    journal: &mut Journal,
    supervisor_pid: u32,
    snapshot_period_ms: u64,
    children: &mut BTreeMap<u32, ObservedChild>,
    probe_error_reported: &mut bool,
    retained_journal_errors: &mut FailureTracker,
) {
    match direct_children(supervisor_pid) {
        Ok(pids) => {
            if *probe_error_reported {
                let event = session_event(
                    "session-children-probe-restored",
                    unix_ns_or_zero(),
                    supervisor_pid,
                );
                append_after_spawn(journal, &event, retained_journal_errors);
            }
            *probe_error_reported = false;
            for pid in pids {
                if let Some(existing) = children.get_mut(&pid) {
                    if existing.catalog_role.is_none() {
                        let observed = classify_child(pid, snapshot_period_ms);
                        if existing.should_reclassify_as(&observed) {
                            let event =
                                child_reclassified_event(supervisor_pid, pid, existing, &observed);
                            append_after_spawn(journal, &event, retained_journal_errors);
                            existing.reclassify_from(&observed);
                        }
                    }
                    continue;
                }
                let mut child = classify_child(pid, snapshot_period_ms);
                let snapshot_transition = child.live_snapshots.capture_now(pid);
                let event = child_started_event(supervisor_pid, pid, &child);
                append_after_spawn(journal, &event, retained_journal_errors);
                if let Some(transition) = snapshot_transition {
                    append_child_live_snapshot_transition(
                        journal,
                        supervisor_pid,
                        pid,
                        &child,
                        transition,
                        retained_journal_errors,
                    );
                }
                children.insert(pid, child);
            }
        }
        Err(error) if !*probe_error_reported => {
            *probe_error_reported = true;
            let event = session_event(
                "session-children-probe-failed",
                unix_ns_or_zero(),
                supervisor_pid,
            )
            .field("error_kind", format!("{:?}", error.kind()))
            .field("raw_os_error", optional_i32(error.raw_os_error()))
            .field("error_hex", hex_bytes(error.to_string().as_bytes()));
            append_after_spawn(journal, &event, retained_journal_errors);
        }
        Err(_) => {}
    }
}

fn child_started_event(supervisor_pid: u32, pid: u32, child: &ObservedChild) -> Event {
    let event = session_event("session-child-observed", unix_ns_or_zero(), supervisor_pid)
        .field("role", &child.role)
        .field("relation", child.relation)
        .field("layer", child.layer)
        .field("identity_source", child.identity_source)
        .field(
            "first_observed_unix_ns",
            system_time_unix_ns(child.first_observed_wall),
        )
        .field("child_pid", pid);
    let event = catalog_event_fields(event, child.catalog_role);
    process::child_event_fields(event, pid)
}

fn append_child_live_snapshot_transition(
    journal: &mut Journal,
    supervisor_pid: u32,
    pid: u32,
    child: &ObservedChild,
    transition: live_snapshot::CaptureTransition,
    retained_journal_errors: &mut FailureTracker,
) {
    let event = session_event(
        transition.session_event_name(),
        unix_ns_or_zero(),
        supervisor_pid,
    )
    .field("role", &child.role)
    .field("relation", child.relation)
    .field("layer", child.layer)
    .field("identity_source", child.identity_source)
    .field("child_pid", pid);
    let event = catalog_event_fields(event, child.catalog_role);
    let event = child.live_snapshots.summary_event_fields(event);
    append_after_spawn(journal, &event, retained_journal_errors);
}

fn child_reclassified_event(
    supervisor_pid: u32,
    pid: u32,
    previous: &ObservedChild,
    observed: &ObservedChild,
) -> Event {
    let event = session_event(
        "session-child-reclassified",
        unix_ns_or_zero(),
        supervisor_pid,
    )
    .field("child_pid", pid)
    .field("role", &observed.role)
    .field("relation", observed.relation)
    .field("layer", observed.layer)
    .field("identity_source", observed.identity_source)
    .field("previous_role", &previous.role)
    .field("previous_relation", previous.relation)
    .field("previous_layer", previous.layer)
    .field("previous_identity_source", previous.identity_source)
    .field(
        "first_observed_unix_ns",
        system_time_unix_ns(previous.first_observed_wall),
    )
    .field(
        "elapsed_since_first_observed_ns",
        previous.first_observed.elapsed().as_nanos(),
    );
    let event = catalog_event_fields(event, observed.catalog_role);
    process::child_event_fields(event, pid)
}

fn classify_child(pid: u32, fallback_snapshot_period_ms: u64) -> ObservedChild {
    let root = PathBuf::from(format!("/proc/{pid}"));
    let executable = fs::read_link(root.join("exe"));
    let cmdline = fs::read(root.join("cmdline")).unwrap_or_default();
    let comm = fs::read(root.join("comm")).ok();
    if let Some(role) = comm
        .as_deref()
        .and_then(|comm| catalog::identify_process_owner(comm).ok().flatten())
    {
        return ObservedChild::classified(
            pid,
            role.name(),
            role,
            "direct-process-owner",
            "supervisor-process-name",
        );
    }
    let basename = executable
        .as_ref()
        .ok()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str());
    if basename == Some("dmc2-process-supervisor") {
        return match process_supervisor_role(&cmdline).and_then(|name| {
            catalog::role(OsStr::new(&name))
                .ok()
                .map(|role| (name, role))
        }) {
            Some((name, role)) => ObservedChild::classified(
                pid,
                name,
                role,
                "direct-process-owner",
                "supervisor-command-line-role",
            ),
            None => ObservedChild::unknown(
                pid,
                "process-supervisor:unknown",
                "direct-process-owner",
                "unmatched",
                fallback_snapshot_period_ms,
            ),
        };
    }
    match catalog::identify_process(executable.as_deref().ok(), &cmdline, comm.as_deref()) {
        Ok(Some((role, source))) => {
            ObservedChild::classified(pid, role.name(), role, "catalogued-workload", source.name())
        }
        Ok(None) | Err(_) => match basename {
            Some(name) => ObservedChild::unknown(
                pid,
                format!("uncatalogued:{name}"),
                "uncatalogued-descendant",
                "proc-executable-basename-only",
                fallback_snapshot_period_ms,
            ),
            None => ObservedChild::unknown(
                pid,
                "unknown",
                "unidentified-descendant",
                "unmatched",
                fallback_snapshot_period_ms,
            ),
        },
    }
}

fn catalog_event_fields(event: Event, role: Option<ProcessRole>) -> Event {
    match role {
        Some(role) => event
            .field("catalog_state", "matched")
            .field("catalog_program", role.program())
            .field("catalog_launch_site", role.launch_site())
            .field("catalog_ownership", role.ownership().name())
            .field("catalog_criticality", role.criticality().name())
            .field("catalog_backtrace", role.backtrace().name())
            .field("catalog_core_dump_policy", role.core_dump_policy().name())
            .field("catalog_owner_comm", role.owner_comm())
            .field(
                "catalog_live_snapshot_period_ms",
                role.live_snapshot_period_ms(),
            )
            .field(
                "catalog_argument_placement",
                role.argument_placement().name(),
            ),
        None => event.field("catalog_state", "unmatched"),
    }
}

fn process_supervisor_role(cmdline: &[u8]) -> Option<String> {
    let fields = cmdline
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    fields
        .windows(2)
        .find(|pair| pair[0] == b"--role")
        .map(|pair| String::from_utf8_lossy(pair[1]).into_owned())
}

fn direct_children(supervisor_pid: u32) -> io::Result<Vec<u32>> {
    let path = format!("/proc/self/task/{supervisor_pid}/children");
    let contents = fs::read_to_string(path)?;
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
    // SAFETY: PR_SET_CHILD_SUBREAPER consumes an integer flag and does not
    // dereference any argument or retain process memory.
    if unsafe { prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn verify_child_subreaper() -> io::Result<()> {
    const PR_GET_CHILD_SUBREAPER: c_int = 37;
    let mut enabled = 0_i32;
    // SAFETY: PR_GET_CHILD_SUBREAPER writes one c_int through the valid local
    // pointer and does not retain it.
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

fn session_event(kind: &'static str, unix_ns: u128, supervisor_pid: u32) -> Event {
    Event::new(kind, unix_ns, supervisor_pid).field("tracker", "session-subreaper")
}

fn append_after_spawn(journal: &mut Journal, event: &Event, failures: &mut FailureTracker) {
    match journal.append(event) {
        Ok(()) => failures.record_success(),
        Err(error) => {
            eprintln!(
                "dmc2-session-supervisor: lifecycle_journal_append=failed error={error} fallback_event={}",
                event.render()
            );
            failures.record_failure(error);
        }
    }
}

fn supervisor_exit_code(status: ExitStatus) -> u8 {
    if let Some(code) = status.code() {
        return u8::try_from(code).unwrap_or(TRACKING_FAILURE_EXIT_CODE);
    }
    status
        .signal()
        .and_then(|signal| u8::try_from(128_i32.saturating_add(signal)).ok())
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

fn unix_ns_or_zero() -> u128 {
    unix_ns().unwrap_or(0)
}

fn system_time_unix_ns(value: SystemTime) -> u128 {
    value
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

struct ObservedChild {
    role: String,
    catalog_role: Option<ProcessRole>,
    relation: &'static str,
    layer: &'static str,
    identity_source: &'static str,
    first_observed: Instant,
    first_observed_wall: SystemTime,
    working_directory: WorkingDirectoryEvidence,
    live_snapshots: live_snapshot::Tracker,
}

impl ObservedChild {
    fn classified(
        pid: u32,
        name: impl Into<String>,
        role: ProcessRole,
        layer: &'static str,
        identity_source: &'static str,
    ) -> Self {
        let first_observed = Instant::now();
        let first_observed_wall = SystemTime::now();
        Self {
            role: name.into(),
            catalog_role: Some(role),
            relation: "adopted-session-descendant",
            layer,
            identity_source,
            first_observed,
            first_observed_wall,
            working_directory: WorkingDirectoryEvidence::for_process(pid),
            live_snapshots: live_snapshot::Tracker::deferred(role.live_snapshot_period_ms()),
        }
    }

    fn unknown(
        pid: u32,
        name: impl Into<String>,
        layer: &'static str,
        identity_source: &'static str,
        fallback_snapshot_period_ms: u64,
    ) -> Self {
        let first_observed = Instant::now();
        let first_observed_wall = SystemTime::now();
        Self {
            role: name.into(),
            catalog_role: None,
            relation: "adopted-session-descendant",
            layer,
            identity_source,
            first_observed,
            first_observed_wall,
            working_directory: WorkingDirectoryEvidence::for_process(pid),
            live_snapshots: live_snapshot::Tracker::deferred(fallback_snapshot_period_ms),
        }
    }

    fn should_reclassify_as(&self, observed: &Self) -> bool {
        self.catalog_role.is_none() && observed.catalog_role.is_some()
    }

    fn reclassify_from(&mut self, observed: &Self) {
        self.role.clone_from(&observed.role);
        self.catalog_role = observed.catalog_role;
        self.relation = observed.relation;
        self.layer = observed.layer;
        self.identity_source = observed.identity_source;
        if let Some(role) = self.catalog_role {
            self.live_snapshots
                .set_period_ms(role.live_snapshot_period_ms());
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_process_supervisor_role_from_nul_separated_cmdline() {
        assert_eq!(
            process_supervisor_role(b"/bin/supervisor\0--role\0milltask\0--journal\0x\0"),
            Some("milltask".to_owned())
        );
    }

    #[test]
    fn reports_no_role_when_option_is_absent() {
        assert_eq!(
            process_supervisor_role(b"/bin/supervisor\0--journal\0x\0"),
            None
        );
    }

    #[test]
    fn root_probe_reports_a_terminal_child_that_has_not_been_reaped() {
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg("exit 23")
            .spawn()
            .expect("spawn root-probe test child");
        let initial = wait::observe(&child).expect("retain terminal test child");
        assert_eq!(initial.status, 23);

        let observation = SessionRootObservation::capture(u32::MAX, child.id(), false);
        let event = observation
            .event_fields(Event::new("root-probe-test", 1, 2), child.id())
            .render();
        assert!(event.contains("\tsession_root_state=terminal-pending-at-probe\t"));
        assert!(event.contains("\tsession_root_terminal_at_child_observation=true\t"));
        assert!(event.contains("\tsession_root_waitid_code_name=CLD_EXITED\t"));
        assert!(event.contains("\tsession_root_waitid_status=23\t"));

        let reaped = wait::reap(&mut child).expect("reap root-probe test child");
        assert_eq!(reaped.status.code(), Some(23));
    }
}
