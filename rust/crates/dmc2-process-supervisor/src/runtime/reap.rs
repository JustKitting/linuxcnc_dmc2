use std::io;
use std::thread;

use crate::catalog::ProcessRole;
use crate::journal::{FailureTracker, Journal};
use crate::reap_degradation::{self, ReapDegradation};
use crate::signal_evidence;
use crate::wait::{self, TerminalObservation, WaitEvidence};

use super::{
    append_after_spawn, append_caught_signal_observations, base_event, unix_ns_or_zero,
    OBSERVATION_PERIOD,
};

pub(super) fn reap_retained_child(
    journal: &mut Journal,
    role: ProcessRole,
    child_pid: u32,
    observation: TerminalObservation,
    caught_signals: &mut signal_evidence::Tracker,
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
                                    base_event(
                                        "retained-terminal-reap-restored",
                                        unix_ns_or_zero(),
                                        std::process::id(),
                                        role,
                                    )
                                    .field("child_ownership_released", true)
                                    .field("reap_wait4_result", "terminal-reaped"),
                                ),
                            ),
                        );
                        append_after_spawn(
                            journal,
                            &event,
                            role,
                            child_pid,
                            retained_journal_errors,
                        );
                    }
                }
                return (evidence, degradation);
            }
            Ok(None) => {
                let error = reap_degradation::terminal_became_nonterminal_error(child_pid);
                record_retained_reap_failure(
                    journal,
                    role,
                    child_pid,
                    observation,
                    &error,
                    &mut degradation,
                    retained_journal_errors,
                );
            }
            Err(error) => {
                record_retained_reap_failure(
                    journal,
                    role,
                    child_pid,
                    observation,
                    &error,
                    &mut degradation,
                    retained_journal_errors,
                );
            }
        }
        append_caught_signal_observations(
            journal,
            role,
            child_pid,
            caught_signals,
            retained_journal_errors,
        );
        thread::sleep(OBSERVATION_PERIOD);
    }
}

fn record_retained_reap_failure(
    journal: &mut Journal,
    role: ProcessRole,
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
                base_event(
                    "retained-terminal-reap-failed",
                    unix_ns_or_zero(),
                    std::process::id(),
                    role,
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
    append_after_spawn(journal, &event, role, child_pid, retained_journal_errors);
}
