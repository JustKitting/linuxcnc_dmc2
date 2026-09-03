use std::collections::BTreeMap;
use std::io;
use std::thread;

use crate::journal::{FailureTracker, Journal};
use crate::reap_degradation::{self, ReapDegradation};
use crate::wait::{self, TerminalObservation, WaitEvidence};

use super::{
    append_after_spawn, append_child_live_snapshot_transition, refresh_session_children,
    session_event, unix_ns_or_zero, ObservedChild, OBSERVATION_PERIOD,
};

pub(super) fn reap_retained_session_child(
    journal: &mut Journal,
    supervisor_pid: u32,
    child_pid: u32,
    observation: TerminalObservation,
    snapshot_period_ms: u64,
    children: &mut BTreeMap<u32, ObservedChild>,
    children_probe_error_reported: &mut bool,
    retained_journal_errors: &mut FailureTracker,
) -> (WaitEvidence, Option<ReapDegradation>) {
    let mut degradation: Option<ReapDegradation> = None;
    loop {
        let result = wait::reap_pid_nonblocking(child_pid);
        match result {
            Ok(Some(evidence)) => {
                if let Some(degradation) = &mut degradation {
                    if degradation.record_success() {
                        let event = degradation.summary_event_fields(
                            evidence.event_fields(
                                observation.event_fields(
                                    session_event(
                                        "session-retained-terminal-reap-restored",
                                        unix_ns_or_zero(),
                                        supervisor_pid,
                                    )
                                    .field("child_ownership_released", true)
                                    .field("reap_wait4_result", "terminal-reaped"),
                                ),
                            ),
                        );
                        append_after_spawn(journal, &event, retained_journal_errors);
                    }
                }
                return (evidence, degradation);
            }
            Ok(None) => {
                let error = reap_degradation::terminal_became_nonterminal_error(child_pid);
                record_session_reap_failure(
                    journal,
                    supervisor_pid,
                    child_pid,
                    observation,
                    &error,
                    &mut degradation,
                    retained_journal_errors,
                );
            }
            Err(error) => {
                record_session_reap_failure(
                    journal,
                    supervisor_pid,
                    child_pid,
                    observation,
                    &error,
                    &mut degradation,
                    retained_journal_errors,
                );
            }
        }

        refresh_session_children(
            journal,
            supervisor_pid,
            snapshot_period_ms,
            children,
            children_probe_error_reported,
            retained_journal_errors,
        );
        for (pid, child) in &mut *children {
            if *pid == child_pid {
                continue;
            }
            if let Some(transition) = child.live_snapshots.capture_if_due(*pid) {
                append_child_live_snapshot_transition(
                    journal,
                    supervisor_pid,
                    *pid,
                    child,
                    transition,
                    retained_journal_errors,
                );
            }
        }
        thread::sleep(OBSERVATION_PERIOD);
    }
}

fn record_session_reap_failure(
    journal: &mut Journal,
    supervisor_pid: u32,
    child_pid: u32,
    observation: TerminalObservation,
    error: &io::Error,
    degradation: &mut Option<ReapDegradation>,
    retained_journal_errors: &mut FailureTracker,
) {
    let changed = match degradation {
        Some(degradation) => degradation.record_failure(error),
        None => {
            *degradation = Some(ReapDegradation::new(error));
            true
        }
    };
    if !changed {
        return;
    }
    let Some(degradation) = degradation.as_ref() else {
        return;
    };
    let event = degradation.event_fields(
        degradation.summary_event_fields(
            observation.event_fields(
                session_event(
                    "session-retained-terminal-reap-failed",
                    unix_ns_or_zero(),
                    supervisor_pid,
                )
                .field("child_pid", child_pid)
                .field("child_ownership_released", false)
                .field("terminal_waitid_status_retained", true)
                .field(
                    "terminal_acquisition_fallback",
                    "retry-exact-pid-wait4-wnohang",
                ),
            ),
        ),
        error,
    );
    append_after_spawn(journal, &event, retained_journal_errors);
}
