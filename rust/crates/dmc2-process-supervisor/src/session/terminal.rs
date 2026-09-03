use std::collections::BTreeMap;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::time::SystemTime;

use crate::backtrace::{self, BacktraceEvidence};
use crate::catalog::{BacktraceKind, ProcessRole};
use crate::core_artifact::CoreArtifactEvidence;
use crate::event::{hex_bytes, Event};
use crate::process;
use crate::wait::{self, TerminalObservation, WaitEvidence};
use crate::wait_degradation::WaitDegradation;

use super::{
    catalog_event_fields, direct_children, optional_i32, session_event, system_time_unix_ns,
    unix_ns_or_zero, ObservedChild,
};

pub(super) fn child_terminal_observed_event(
    supervisor_pid: u32,
    linuxcnc_pid: u32,
    session_root: &SessionRootObservation,
    child: Option<&ObservedChild>,
    terminal_identity: &ObservedChild,
    observation_state: &'static str,
    observation: TerminalObservation,
    children: &BTreeMap<u32, ObservedChild>,
) -> Event {
    let event = session_event(
        "session-child-terminal-observed",
        unix_ns_or_zero(),
        supervisor_pid,
    )
    .field("role", child.map_or("unknown", |child| child.role.as_str()))
    .field(
        "relation",
        child.map_or("unobserved-session-descendant", |child| child.relation),
    )
    .field("layer", child.map_or("unobserved", |child| child.layer))
    .field(
        "identity_source",
        child.map_or("terminal-proc-snapshot", |child| child.identity_source),
    )
    .field("terminal_role", &terminal_identity.role)
    .field("terminal_layer", terminal_identity.layer)
    .field(
        "terminal_identity_source",
        terminal_identity.identity_source,
    )
    .field(
        "terminal_identity_matches_initial",
        child.is_some_and(|child| child.role == terminal_identity.role),
    )
    .field(
        "first_observed_unix_ns",
        child.map_or_else(
            || "NONE".to_owned(),
            |child| system_time_unix_ns(child.first_observed_wall).to_string(),
        ),
    )
    .field("observation_state", observation_state)
    .field("snapshot_phase", "terminal-before-reap");
    let event = session_root.event_fields(event, linuxcnc_pid);
    let event = catalog_event_fields(event, child.and_then(|child| child.catalog_role));
    let event = observation.event_fields(event);
    let event = tracked_children_event_fields(event, children, observation.pid, child);
    let event = process::terminal_child_event_fields(event, observation.pid);
    match child {
        Some(child) => child.live_snapshots.full_event_fields(event),
        None => event.field("last_live_snapshot_state", "untracked-terminal-only"),
    }
}

pub(super) fn child_terminal_reaped_fallback_event(
    supervisor_pid: u32,
    linuxcnc_pid: u32,
    session_root: &SessionRootObservation,
    child: Option<&ObservedChild>,
    observation_state: &'static str,
    evidence: WaitEvidence,
    children: &BTreeMap<u32, ObservedChild>,
    degradation: &WaitDegradation,
) -> Event {
    let event = session_event(
        "session-child-terminal-reaped-fallback",
        unix_ns_or_zero(),
        supervisor_pid,
    )
    .field("role", child.map_or("unknown", |child| child.role.as_str()))
    .field(
        "relation",
        child.map_or("unobserved-session-descendant", |child| child.relation),
    )
    .field("layer", child.map_or("unobserved", |child| child.layer))
    .field(
        "identity_source",
        child.map_or("unavailable-after-fallback-reap", |child| {
            child.identity_source
        }),
    )
    .field("observation_state", observation_state)
    .field("snapshot_phase", "post-reap-fallback")
    .field(
        "terminal_acquisition_method",
        "wait4-p-all-wnohang-after-waitid-error",
    )
    .field(
        "terminal_proc_snapshot_state",
        "not-attempted-after-reap-to-avoid-pid-reuse",
    )
    .field(
        "first_observed_unix_ns",
        child.map_or_else(
            || "NONE".to_owned(),
            |child| system_time_unix_ns(child.first_observed_wall).to_string(),
        ),
    );
    let event = session_root.event_fields(event, linuxcnc_pid);
    let event = catalog_event_fields(event, child.and_then(|child| child.catalog_role));
    let event = evidence.event_fields(event);
    let event =
        tracked_children_after_fallback_reap_event_fields(event, children, evidence.pid, child);
    let event = match child {
        Some(child) => child.live_snapshots.full_event_fields(event),
        None => event.field("last_live_snapshot_state", "untracked-terminal-only"),
    };
    degradation.summary_event_fields(event)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn child_terminated_event(
    supervisor_pid: u32,
    linuxcnc_pid: u32,
    session_root: &SessionRootObservation,
    child: Option<&ObservedChild>,
    observation_state: &'static str,
    terminated_unix_ns: u128,
    observation: Option<TerminalObservation>,
    evidence: WaitEvidence,
    backtrace: &BacktraceEvidence,
    core_artifact: &CoreArtifactEvidence,
) -> Event {
    let (role, relation, observed_ns) = match child {
        Some(child) => (
            child.role.as_str(),
            child.relation,
            child.first_observed.elapsed().as_nanos(),
        ),
        None => ("unknown", "unobserved-session-descendant", 0),
    };
    let event = session_event(
        "session-child-terminated",
        terminated_unix_ns,
        supervisor_pid,
    )
    .field("role", role)
    .field("relation", relation)
    .field("layer", child.map_or("unobserved", |child| child.layer))
    .field(
        "identity_source",
        child.map_or("unavailable-after-reap", |child| child.identity_source),
    )
    .field("observation_state", observation_state)
    .field(
        "first_observed_unix_ns",
        child.map_or_else(
            || "NONE".to_owned(),
            |child| system_time_unix_ns(child.first_observed_wall).to_string(),
        ),
    )
    .field("elapsed_since_observed_ns", observed_ns);
    let event = match observation {
        Some(observation) => observation.event_fields(
            event
                .field("terminal_acquisition_method", "waitid-wnowait-then-wait4")
                .field("waitid_available", true)
                .field("waitid_wait4_consistent", observation.agrees_with(evidence)),
        ),
        None => event
            .field(
                "terminal_acquisition_method",
                "wait4-p-all-wnohang-after-waitid-error",
            )
            .field("waitid_available", false)
            .field("waitid_wait4_consistent", "NOT_COMPARABLE"),
    };
    let event = session_root.event_fields(event, linuxcnc_pid);
    let event = catalog_event_fields(event, child.and_then(|child| child.catalog_role));
    let event = evidence.event_fields(event);
    let event = backtrace.event_fields(event);
    let event = core_artifact.event_fields(event);
    let event = match child {
        Some(child) => child.live_snapshots.summary_event_fields(event),
        None => event.field("last_live_snapshot_state", "untracked-terminal-only"),
    };
    let outcome = match (
        evidence.status.signal(),
        evidence.status.code(),
        backtrace.reported_signal(),
    ) {
        (_, _, Some(8 | 11)) => "linuxcnc-handled-fatal-signal",
        (Some(_), _, _) => "kernel-signal-termination",
        (None, Some(0), _) => "zero-exit",
        (None, Some(_), _) => "nonzero-exit",
        _ => "unknown-wait-status",
    };
    event.field("outcome", outcome)
}

pub(super) fn session_backtrace(
    journal_path: &Path,
    child: &ObservedChild,
    child_pid: u32,
    process_not_before: SystemTime,
    exit_unix_ns: u128,
) -> BacktraceEvidence {
    if child.layer != "catalogued-workload" {
        return BacktraceEvidence::NotApplicable;
    }
    match child.catalog_role.map(ProcessRole::backtrace) {
        Some(BacktraceKind::LinuxCncTask) => {
            backtrace::capture(journal_path, child_pid, process_not_before, exit_unix_ns)
        }
        Some(BacktraceKind::None) | None => BacktraceEvidence::NotApplicable,
    }
}

#[derive(Debug)]
pub(super) enum SessionRootObservation {
    Nonterminal {
        observed_unix_ns: u128,
    },
    TerminalPending {
        observed_unix_ns: u128,
        waitid: TerminalObservation,
    },
    TerminalEvent {
        observed_unix_ns: u128,
    },
    AlreadyReaped {
        observed_unix_ns: u128,
    },
    ProbeFailed {
        observed_unix_ns: u128,
        error_kind: String,
        raw_os_error: Option<i32>,
        error_hex: String,
    },
}

impl SessionRootObservation {
    pub(super) fn capture(terminal_pid: u32, linuxcnc_pid: u32, status_captured: bool) -> Self {
        let observed_unix_ns = unix_ns_or_zero();
        if terminal_pid == linuxcnc_pid {
            return Self::TerminalEvent { observed_unix_ns };
        }
        if status_captured {
            return Self::AlreadyReaped { observed_unix_ns };
        }
        match wait::observe_pid_nonblocking(linuxcnc_pid) {
            Ok(None) => Self::Nonterminal { observed_unix_ns },
            Ok(Some(waitid)) => Self::TerminalPending {
                observed_unix_ns,
                waitid,
            },
            Err(error) => Self::ProbeFailed {
                observed_unix_ns,
                error_kind: format!("{:?}", error.kind()),
                raw_os_error: error.raw_os_error(),
                error_hex: hex_bytes(error.to_string().as_bytes()),
            },
        }
    }

    pub(super) fn event_fields(&self, event: Event, linuxcnc_pid: u32) -> Event {
        let event = event
            .field("linuxcnc_pid", linuxcnc_pid)
            .field("session_root_observation_unix_ns", self.observed_unix_ns());
        match self {
            Self::Nonterminal { .. } => event
                .field("session_root_state", "nonterminal-at-probe")
                .field(
                    "session_root_observation_method",
                    "waitid-p-pid-wnohang-wnowait",
                )
                .field("session_root_terminal_at_child_observation", "false"),
            Self::TerminalPending { waitid, .. } => event
                .field("session_root_state", "terminal-pending-at-probe")
                .field(
                    "session_root_observation_method",
                    "waitid-p-pid-wnohang-wnowait",
                )
                .field("session_root_terminal_at_child_observation", "true")
                .field("session_root_waitid_pid", waitid.pid)
                .field("session_root_waitid_signal", waitid.signal)
                .field("session_root_waitid_error", waitid.error)
                .field("session_root_waitid_code", waitid.code)
                .field("session_root_waitid_code_name", waitid.code_name())
                .field("session_root_waitid_uid", waitid.uid)
                .field("session_root_waitid_status", waitid.status)
                .field("session_root_waitid_user_ticks", waitid.user_ticks)
                .field("session_root_waitid_system_ticks", waitid.system_ticks),
            Self::TerminalEvent { .. } => event
                .field("session_root_state", "terminal-event")
                .field(
                    "session_root_observation_method",
                    "current-waitid-p-all-wnowait",
                )
                .field("session_root_terminal_at_child_observation", "true"),
            Self::AlreadyReaped { .. } => event
                .field("session_root_state", "already-reaped")
                .field("session_root_observation_method", "retained-wait4-status")
                .field("session_root_terminal_at_child_observation", "true"),
            Self::ProbeFailed {
                error_kind,
                raw_os_error,
                error_hex,
                ..
            } => event
                .field("session_root_state", "probe-failed")
                .field(
                    "session_root_observation_method",
                    "waitid-p-pid-wnohang-wnowait",
                )
                .field("session_root_terminal_at_child_observation", "UNKNOWN")
                .field("session_root_probe_error_kind", error_kind)
                .field(
                    "session_root_probe_raw_os_error",
                    optional_i32(*raw_os_error),
                )
                .field("session_root_probe_error_hex", error_hex),
        }
    }

    fn observed_unix_ns(&self) -> u128 {
        match self {
            Self::Nonterminal { observed_unix_ns }
            | Self::TerminalPending {
                observed_unix_ns, ..
            }
            | Self::TerminalEvent { observed_unix_ns }
            | Self::AlreadyReaped { observed_unix_ns }
            | Self::ProbeFailed {
                observed_unix_ns, ..
            } => *observed_unix_ns,
        }
    }
}

fn tracked_children_event_fields(
    event: Event,
    children: &BTreeMap<u32, ObservedChild>,
    terminal_pid: u32,
    terminal_child: Option<&ObservedChild>,
) -> Event {
    let mut tracked = children
        .iter()
        .map(|(pid, child)| format!("{pid}:{}", child.role))
        .collect::<Vec<_>>();
    if !children.contains_key(&terminal_pid) {
        tracked.push(format!(
            "{terminal_pid}:{}",
            terminal_child.map_or("unknown", |child| child.role.as_str())
        ));
        tracked.sort();
    }
    let tracked = tracked.join(",");
    let tracked_count = children.len() + usize::from(!children.contains_key(&terminal_pid));
    match direct_children(std::process::id()) {
        Ok(kernel_children) => event
            .field("tracked_children_before_reap", tracked_count)
            .field(
                "tracked_children_identity_hex",
                hex_bytes(tracked.as_bytes()),
            )
            .field("kernel_children_probe_state", "captured")
            .field("kernel_children_before_reap", kernel_children.len())
            .field(
                "kernel_children_pid_list",
                kernel_children
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        Err(error) => event
            .field("tracked_children_before_reap", tracked_count)
            .field(
                "tracked_children_identity_hex",
                hex_bytes(tracked.as_bytes()),
            )
            .field("kernel_children_probe_state", "failed")
            .field("kernel_children_error_kind", format!("{:?}", error.kind()))
            .field(
                "kernel_children_raw_os_error",
                optional_i32(error.raw_os_error()),
            )
            .field(
                "kernel_children_error_hex",
                hex_bytes(error.to_string().as_bytes()),
            ),
    }
}

fn tracked_children_after_fallback_reap_event_fields(
    event: Event,
    children: &BTreeMap<u32, ObservedChild>,
    terminal_pid: u32,
    terminal_child: Option<&ObservedChild>,
) -> Event {
    let mut tracked = children
        .iter()
        .map(|(pid, child)| format!("{pid}:{}", child.role))
        .collect::<Vec<_>>();
    if !children.contains_key(&terminal_pid) {
        tracked.push(format!(
            "{terminal_pid}:{}",
            terminal_child.map_or("unknown", |child| child.role.as_str())
        ));
        tracked.sort();
    }
    let tracked_count = children.len() + usize::from(!children.contains_key(&terminal_pid));
    let tracked = tracked.join(",");
    match direct_children(std::process::id()) {
        Ok(kernel_children) => event
            .field("tracked_children_at_fallback_reap", tracked_count)
            .field(
                "tracked_children_identity_hex",
                hex_bytes(tracked.as_bytes()),
            )
            .field(
                "kernel_children_probe_state",
                "captured-after-fallback-reap",
            )
            .field("kernel_children_after_fallback_reap", kernel_children.len())
            .field(
                "kernel_children_pid_list",
                kernel_children
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        Err(error) => event
            .field("tracked_children_at_fallback_reap", tracked_count)
            .field(
                "tracked_children_identity_hex",
                hex_bytes(tracked.as_bytes()),
            )
            .field("kernel_children_probe_state", "failed-after-fallback-reap")
            .field("kernel_children_error_kind", format!("{:?}", error.kind()))
            .field(
                "kernel_children_raw_os_error",
                optional_i32(error.raw_os_error()),
            )
            .field(
                "kernel_children_error_hex",
                hex_bytes(error.to_string().as_bytes()),
            ),
    }
}
