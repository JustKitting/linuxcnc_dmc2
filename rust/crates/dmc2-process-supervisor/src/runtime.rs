use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant, SystemTime, SystemTimeError, UNIX_EPOCH};

use crate::backtrace::{self, BacktraceEvidence};
use crate::catalog::{BacktraceKind, Ownership, ProcessRole};
use crate::cli::Invocation;
use crate::core_artifact::{self, CoreArtifactEvidence, WorkingDirectoryEvidence};
use crate::event::{encode_arguments, hex_bytes, Event};
use crate::journal::{FailureTracker, Journal};
use crate::limits::CoreDumpPlan;
use crate::live_snapshot;
use crate::process;
use crate::signal_evidence;
use crate::wait::{self, TerminalObservation, WaitEvidence};
use crate::wait_degradation::WaitDegradation;

pub const TRACKING_FAILURE_EXIT_CODE: u8 = 125;
const OBSERVATION_PERIOD: Duration = Duration::from_millis(5);

mod error;
mod reap;

pub use error::SupervisorError;
use reap::reap_retained_child;

enum TerminalAcquisition {
    Retained(TerminalObservation),
    ReapedFallback {
        evidence: WaitEvidence,
        degradation: WaitDegradation,
    },
}

pub fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<u8, SupervisorError> {
    let invocation = Invocation::parse(arguments).map_err(SupervisorError::Cli)?;
    if invocation.role.ownership() != Ownership::DirectChild {
        return Err(SupervisorError::UnsupportedOwnership {
            role: invocation.role,
            ownership: invocation.role.ownership(),
        });
    }
    supervise(invocation)
}

fn supervise(invocation: Invocation) -> Result<u8, SupervisorError> {
    let supervisor_pid = std::process::id();
    let mut journal = Journal::open(&invocation.journal).map_err(SupervisorError::Journal)?;
    if let Err(source) = process::set_process_owner_identity(invocation.role) {
        let event = base_event(
            "owner-identity-failed",
            unix_ns_or_zero(),
            supervisor_pid,
            invocation.role,
        )
        .field("error_kind", format!("{:?}", source.kind()))
        .field("raw_os_error", optional_i32(source.raw_os_error()))
        .field("error_hex", hex_bytes(source.to_string().as_bytes()));
        return match journal.append(&event) {
            Ok(()) => Err(SupervisorError::OwnerIdentity {
                role: invocation.role,
                source,
            }),
            Err(journal) => Err(SupervisorError::OwnerIdentityAndJournal {
                role: invocation.role,
                identity: source,
                journal,
            }),
        };
    }
    let core_dump_plan = match CoreDumpPlan::capture(invocation.role.core_dump_policy()) {
        Ok(plan) => plan,
        Err(source) => {
            let event = base_event(
                "core-limit-plan-failed",
                unix_ns_or_zero(),
                supervisor_pid,
                invocation.role,
            )
            .field("error_kind", format!("{:?}", source.kind()))
            .field("raw_os_error", optional_i32(source.raw_os_error()))
            .field("error_hex", hex_bytes(source.to_string().as_bytes()));
            return match journal.append(&event) {
                Ok(()) => Err(SupervisorError::CoreDumpLimit {
                    role: invocation.role,
                    source,
                }),
                Err(journal) => Err(SupervisorError::CoreDumpLimitAndJournal {
                    role: invocation.role,
                    limit: source,
                    journal,
                }),
            };
        }
    };
    let mut caught_signals = match signal_evidence::Tracker::prepare(invocation.role) {
        Ok(tracker) => tracker,
        Err(source) => {
            let event = signal_evidence::setup_error_event_fields(
                &source,
                base_event(
                    "caught-signal-evidence-setup-failed",
                    unix_ns_or_zero(),
                    supervisor_pid,
                    invocation.role,
                ),
            );
            return match journal.append(&event) {
                Ok(()) => Err(SupervisorError::SignalEvidenceSetup {
                    role: invocation.role,
                    source,
                }),
                Err(journal) => Err(SupervisorError::SignalEvidenceSetupAndJournal {
                    role: invocation.role,
                    setup: source,
                    journal,
                }),
            };
        }
    };
    let supervisor_started_ns = unix_ns().map_err(SupervisorError::Clock)?;
    let recovered_partial_record = journal.recovered_partial_record();
    let journal_path = journal.path().to_path_buf();
    let supervisor_event = base_event(
        "supervisor-started",
        supervisor_started_ns,
        supervisor_pid,
        invocation.role,
    )
    .field("recovered_partial_record", recovered_partial_record)
    .encoded_path_field("journal_path_hex", &journal_path)
    .encoded_os_field("program_hex", &invocation.program)
    .field("argc", invocation.arguments.len())
    .field("argv_hex", encode_arguments(&invocation.arguments));
    let supervisor_event = core_dump_plan.event_fields(supervisor_event);
    let supervisor_event = caught_signals.plan_event_fields(supervisor_event);
    let supervisor_event = process::environment_event_fields(supervisor_event);
    let supervisor_event = process::host_event_fields(supervisor_event);
    let supervisor_event =
        process::executable_event_fields(supervisor_event, Path::new(&invocation.program));
    let supervisor_event = process::child_event_fields(supervisor_event, supervisor_pid);
    journal
        .append(&supervisor_event)
        .map_err(SupervisorError::Journal)?;

    let launched_at_wall = SystemTime::now();
    let launched_at_monotonic = Instant::now();
    let fallback_cwd = WorkingDirectoryEvidence::for_supervisor();
    let mut command = Command::new(&invocation.program);
    command.args(&invocation.arguments);
    core_dump_plan.configure(&mut command);
    if let Err(source) = caught_signals.configure_child(&mut command) {
        let event = signal_evidence::setup_error_event_fields(
            &source,
            base_event(
                "caught-signal-evidence-configuration-failed",
                unix_ns_or_zero(),
                supervisor_pid,
                invocation.role,
            ),
        );
        return match journal.append(&event) {
            Ok(()) => Err(SupervisorError::SignalEvidenceSetup {
                role: invocation.role,
                source,
            }),
            Err(journal) => Err(SupervisorError::SignalEvidenceSetupAndJournal {
                role: invocation.role,
                setup: source,
                journal,
            }),
        };
    }
    let child = match command.spawn() {
        Ok(child) => child,
        Err(source) => {
            let event = base_event(
                "spawn-failed",
                unix_ns_or_zero(),
                supervisor_pid,
                invocation.role,
            )
            .encoded_os_field("program_hex", &invocation.program)
            .field("error_kind", format!("{:?}", source.kind()))
            .field("raw_os_error", optional_i32(source.raw_os_error()))
            .field("error_hex", hex_bytes(source.to_string().as_bytes()));
            return match journal.append(&event) {
                Ok(()) => Err(SupervisorError::Spawn {
                    role: invocation.role,
                    program: invocation.program,
                    source,
                }),
                Err(journal) => Err(SupervisorError::SpawnAndJournal {
                    role: invocation.role,
                    program: invocation.program,
                    spawn: source,
                    journal,
                }),
            };
        }
    };
    caught_signals.parent_after_spawn();
    let child_pid = child.id();
    let mut live_snapshots =
        live_snapshot::Tracker::deferred(invocation.role.live_snapshot_period_ms());
    let initial_snapshot_transition = live_snapshots.capture_now(child_pid);
    let child_cwd = WorkingDirectoryEvidence::for_process(child_pid);
    let start_event = base_event(
        "process-started",
        unix_ns_or_zero(),
        supervisor_pid,
        invocation.role,
    )
    .field("child_pid", child_pid)
    .encoded_os_field("program_hex", &invocation.program)
    .field("argc", invocation.arguments.len())
    .field("argv_hex", encode_arguments(&invocation.arguments));
    let start_event = process::child_event_fields(start_event, child_pid);
    let mut retained_journal_errors = FailureTracker::new();
    append_after_spawn(
        &mut journal,
        &start_event,
        invocation.role,
        child_pid,
        &mut retained_journal_errors,
    );
    append_caught_signal_observations(
        &mut journal,
        invocation.role,
        child_pid,
        &mut caught_signals,
        &mut retained_journal_errors,
    );
    if let Some(transition) = initial_snapshot_transition {
        append_live_snapshot_transition(
            &mut journal,
            invocation.role,
            child_pid,
            &live_snapshots,
            transition,
            &mut retained_journal_errors,
        );
    }

    let mut observation_degradation: Option<WaitDegradation> = None;
    let acquisition = loop {
        append_caught_signal_observations(
            &mut journal,
            invocation.role,
            child_pid,
            &mut caught_signals,
            &mut retained_journal_errors,
        );
        if let Some(degradation) = &mut observation_degradation {
            match wait::reap_pid_nonblocking(child_pid) {
                Ok(Some(evidence)) => {
                    if degradation.record_fallback_success() {
                        let event = degradation.summary_event_fields(
                            base_event(
                                "terminal-fallback-reap-poll-restored",
                                unix_ns_or_zero(),
                                supervisor_pid,
                                invocation.role,
                            )
                            .field("child_pid", child_pid)
                            .field("fallback_wait4_result", "terminal-reaped"),
                        );
                        append_after_spawn(
                            &mut journal,
                            &event,
                            invocation.role,
                            child_pid,
                            &mut retained_journal_errors,
                        );
                    }
                    break TerminalAcquisition::ReapedFallback {
                        evidence,
                        degradation: degradation.clone(),
                    };
                }
                Ok(None) => {
                    if degradation.record_fallback_success() {
                        let event = degradation.summary_event_fields(
                            base_event(
                                "terminal-fallback-reap-poll-restored",
                                unix_ns_or_zero(),
                                supervisor_pid,
                                invocation.role,
                            )
                            .field("child_pid", child_pid)
                            .field("fallback_wait4_result", "child-running"),
                        );
                        append_after_spawn(
                            &mut journal,
                            &event,
                            invocation.role,
                            child_pid,
                            &mut retained_journal_errors,
                        );
                    }
                }
                Err(source) => {
                    if let Some(error) = degradation.record_fallback_failure(&source) {
                        let event = error.fallback_event_fields(
                            degradation.summary_event_fields(
                                base_event(
                                    "terminal-fallback-reap-poll-failed",
                                    unix_ns_or_zero(),
                                    supervisor_pid,
                                    invocation.role,
                                )
                                .field("child_pid", child_pid)
                                .field("child_ownership_released", false),
                            ),
                        );
                        append_after_spawn(
                            &mut journal,
                            &event,
                            invocation.role,
                            child_pid,
                            &mut retained_journal_errors,
                        );
                    }
                }
            }
        } else {
            match wait::observe_pid_nonblocking(child_pid) {
                Ok(Some(observation)) => {
                    break TerminalAcquisition::Retained(observation);
                }
                Ok(None) => {}
                Err(source) => {
                    let degradation = WaitDegradation::new(source);
                    let event = degradation.waitid_event_fields(
                        base_event(
                            "terminal-observe-failed",
                            unix_ns_or_zero(),
                            supervisor_pid,
                            invocation.role,
                        )
                        .field("child_pid", child_pid)
                        .field("child_ownership_released", false)
                        .field("terminal_acquisition_fallback", "wait4-wnohang-reaping"),
                    );
                    let event = live_snapshots.full_event_fields(event);
                    append_after_spawn(
                        &mut journal,
                        &event,
                        invocation.role,
                        child_pid,
                        &mut retained_journal_errors,
                    );
                    observation_degradation = Some(degradation);
                    continue;
                }
            }
        }

        if let Some(transition) = live_snapshots.capture_if_due(child_pid) {
            append_live_snapshot_transition(
                &mut journal,
                invocation.role,
                child_pid,
                &live_snapshots,
                transition,
                &mut retained_journal_errors,
            );
        }
        thread::sleep(OBSERVATION_PERIOD);
    };
    append_caught_signal_observations(
        &mut journal,
        invocation.role,
        child_pid,
        &mut caught_signals,
        &mut retained_journal_errors,
    );
    let exit_ns = unix_ns_or_zero();
    let elapsed_ns = launched_at_monotonic.elapsed().as_nanos();
    let (
        observation,
        evidence,
        observation_degradation,
        reap_degradation,
        backtrace,
        core_artifact,
    ) = match acquisition {
        TerminalAcquisition::Retained(observation) => {
            let terminal_snapshot = base_event(
                "process-terminal-observed",
                exit_ns,
                supervisor_pid,
                invocation.role,
            )
            .field("child_pid", child_pid)
            .field("elapsed_ns", elapsed_ns)
            .field("terminal_acquisition_method", "waitid-wnowait")
            .field("terminal_observation_degraded", false)
            .field("snapshot_phase", "terminal-before-reap");
            let terminal_snapshot = observation.event_fields(terminal_snapshot);
            let terminal_snapshot =
                process::terminal_child_event_fields(terminal_snapshot, child_pid);
            let terminal_snapshot = live_snapshots.full_event_fields(terminal_snapshot);
            let terminal_snapshot = caught_signals.summary_event_fields(terminal_snapshot);
            append_after_spawn(
                &mut journal,
                &terminal_snapshot,
                invocation.role,
                child_pid,
                &mut retained_journal_errors,
            );

            let backtrace = capture_backtrace(
                invocation.role,
                journal.path(),
                child_pid,
                launched_at_wall,
                exit_ns,
            );
            let core_artifact = core_artifact::capture(
                journal.path(),
                child_pid,
                observation.core_dumped(),
                &child_cwd,
                &fallback_cwd,
                launched_at_wall,
                exit_ns,
            );
            let (evidence, reap_degradation) = reap_retained_child(
                &mut journal,
                invocation.role,
                child_pid,
                observation,
                &mut caught_signals,
                &mut retained_journal_errors,
            );
            (
                Some(observation),
                evidence,
                None,
                reap_degradation,
                backtrace,
                core_artifact,
            )
        }
        TerminalAcquisition::ReapedFallback {
            evidence,
            degradation,
        } => {
            let terminal_snapshot = degradation.summary_event_fields(
                base_event(
                    "process-terminal-reaped-fallback",
                    exit_ns,
                    supervisor_pid,
                    invocation.role,
                )
                .field("elapsed_ns", elapsed_ns)
                .field(
                    "terminal_acquisition_method",
                    "wait4-wnohang-after-waitid-error",
                )
                .field("snapshot_phase", "post-reap-fallback")
                .field(
                    "terminal_proc_snapshot_state",
                    "not-attempted-after-reap-to-avoid-pid-reuse",
                ),
            );
            let terminal_snapshot = evidence.event_fields(terminal_snapshot);
            let terminal_snapshot = live_snapshots.full_event_fields(terminal_snapshot);
            let terminal_snapshot = caught_signals.summary_event_fields(terminal_snapshot);
            append_after_spawn(
                &mut journal,
                &terminal_snapshot,
                invocation.role,
                child_pid,
                &mut retained_journal_errors,
            );

            let backtrace = capture_backtrace(
                invocation.role,
                journal.path(),
                child_pid,
                launched_at_wall,
                exit_ns,
            );
            let core_artifact = core_artifact::capture(
                journal.path(),
                child_pid,
                evidence.status.core_dumped(),
                &child_cwd,
                &fallback_cwd,
                launched_at_wall,
                exit_ns,
            );
            (
                None,
                evidence,
                Some(degradation),
                None,
                backtrace,
                core_artifact,
            )
        }
    };
    let event = termination_event(
        supervisor_pid,
        invocation.role,
        exit_ns,
        elapsed_ns,
        observation,
        evidence,
        &backtrace,
        &core_artifact,
        &caught_signals,
    );
    let event = live_snapshots.summary_event_fields(event);
    let event = caught_signals.summary_event_fields(event);
    let event = match &observation_degradation {
        Some(degradation) => degradation.summary_event_fields(event),
        None => event.field("terminal_observation_degraded", false),
    };
    let event = match &reap_degradation {
        Some(degradation) => degradation.summary_event_fields(event),
        None => event.field("terminal_reap_degraded", false),
    };
    let event = retained_journal_errors.summary_event_fields(event);
    append_after_spawn(
        &mut journal,
        &event,
        invocation.role,
        child_pid,
        &mut retained_journal_errors,
    );
    let signal_failures = caught_signals.infrastructure_failures();
    if let Some(degradation) = observation_degradation {
        let first_journal_failure = retained_journal_errors.take_first();
        return Err(SupervisorError::TerminalObservationDegraded {
            role: invocation.role,
            child_pid,
            waitid_error_kind: degradation.waitid_error().kind,
            waitid_raw_os_error: degradation.waitid_error().raw_os_error,
            waitid_error: degradation.waitid_error().detail.clone(),
            fallback_wait4_failures: degradation.fallback_wait4_failures(),
            first_journal_failure,
            additional_journal_failures: retained_journal_errors.additional_failures(),
            signal_failures,
        });
    }
    if let Some(degradation) = reap_degradation {
        let first_journal_failure = retained_journal_errors.take_first();
        return Err(SupervisorError::TerminalReapDegraded {
            role: invocation.role,
            child_pid,
            first_error_kind: degradation.first_error().kind,
            first_raw_os_error: degradation.first_error().raw_os_error,
            first_error: degradation.first_error().detail.clone(),
            wait4_failures: degradation.failures(),
            first_journal_failure,
            additional_journal_failures: retained_journal_errors.additional_failures(),
            signal_failures,
        });
    }
    match (retained_journal_errors.take_first(), signal_failures) {
        (Some(first), Some(signal)) => {
            return Err(SupervisorError::JournalAndSignalEvidenceAfterChildSpawn {
                first,
                additional_journal_failures: retained_journal_errors.additional_failures(),
                signal,
            });
        }
        (Some(first), None) => {
            return Err(SupervisorError::JournalAfterChildSpawn {
                first,
                additional_failures: retained_journal_errors.additional_failures(),
            });
        }
        (None, Some(failures)) => {
            return Err(SupervisorError::SignalEvidenceAfterChildSpawn {
                role: invocation.role,
                child_pid,
                failures,
            });
        }
        (None, None) => {}
    }

    Ok(supervisor_exit_code(evidence.status))
}

fn append_caught_signal_observations(
    journal: &mut Journal,
    role: ProcessRole,
    child_pid: u32,
    tracker: &mut signal_evidence::Tracker,
    retained_journal_errors: &mut FailureTracker,
) {
    for observation in tracker.drain(child_pid) {
        let event = base_event(
            signal_evidence::observation_event_name(&observation),
            unix_ns_or_zero(),
            std::process::id(),
            role,
        )
        .field("child_pid", child_pid);
        let event = signal_evidence::observation_event(&observation, event);
        append_after_spawn(journal, &event, role, child_pid, retained_journal_errors);
    }
}

fn append_live_snapshot_transition(
    journal: &mut Journal,
    role: ProcessRole,
    child_pid: u32,
    tracker: &live_snapshot::Tracker,
    transition: live_snapshot::CaptureTransition,
    retained_journal_errors: &mut FailureTracker,
) {
    let event = base_event(
        transition.direct_event_name(),
        unix_ns_or_zero(),
        std::process::id(),
        role,
    )
    .field("child_pid", child_pid);
    let event = tracker.summary_event_fields(event);
    append_after_spawn(journal, &event, role, child_pid, retained_journal_errors);
}

fn base_event(kind: &'static str, unix_ns: u128, supervisor_pid: u32, role: ProcessRole) -> Event {
    Event::new(kind, unix_ns, supervisor_pid)
        .field("role", role.name())
        .field("launch_site", role.launch_site())
        .field("ownership", role.ownership().name())
        .field("criticality", role.criticality().name())
        .field("backtrace_contract", role.backtrace().name())
        .field(
            "caught_signal_evidence_contract",
            role.caught_signal_evidence().name(),
        )
        .field("core_dump_policy", role.core_dump_policy().name())
        .field("owner_comm", role.owner_comm())
        .field(
            "catalog_live_snapshot_period_ms",
            role.live_snapshot_period_ms(),
        )
        .field("argument_placement", role.argument_placement().name())
}

fn capture_backtrace(
    role: ProcessRole,
    journal_path: &Path,
    child_pid: u32,
    launched_at_wall: SystemTime,
    exit_ns: u128,
) -> BacktraceEvidence {
    match role.backtrace() {
        BacktraceKind::None => BacktraceEvidence::NotApplicable,
        BacktraceKind::LinuxCncTask => {
            backtrace::capture(journal_path, child_pid, launched_at_wall, exit_ns)
        }
    }
}

fn termination_event(
    supervisor_pid: u32,
    role: ProcessRole,
    exit_ns: u128,
    elapsed_ns: u128,
    observation: Option<TerminalObservation>,
    evidence: WaitEvidence,
    backtrace: &BacktraceEvidence,
    core_artifact: &CoreArtifactEvidence,
    caught_signals: &signal_evidence::Tracker,
) -> Event {
    let event = base_event("process-terminated", exit_ns, supervisor_pid, role)
        .field("elapsed_ns", elapsed_ns);
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
                "wait4-wnohang-after-waitid-error",
            )
            .field("waitid_available", false)
            .field("waitid_wait4_consistent", "NOT_COMPARABLE"),
    };
    let event = evidence.event_fields(event);
    let event = backtrace.event_fields(event);
    let event = core_artifact.event_fields(event);

    let outcome = match (
        evidence.status.signal(),
        evidence.status.code(),
        backtrace.reported_signal(),
    ) {
        (_, _, Some(8 | 11)) => "linuxcnc-handled-fatal-signal",
        (Some(_), _, _) => "kernel-signal-termination",
        (None, Some(0), _) => caught_signals.zero_exit_outcome(),
        (None, Some(_), _) => "nonzero-exit",
        _ => "unknown-wait-status",
    };
    event.field("outcome", outcome)
}

fn append_after_spawn(
    journal: &mut Journal,
    event: &Event,
    role: ProcessRole,
    child_pid: u32,
    failures: &mut FailureTracker,
) {
    if let Err(error) = journal.append(event) {
        eprintln!(
            "dmc2-process-supervisor: role={} child_pid={child_pid} lifecycle_journal_append=failed error={error} fallback_event={}",
            role.name(),
            event.render()
        );
        failures.record_failure(error);
    } else {
        failures.record_success();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirrors_normal_child_status() {
        assert_eq!(supervisor_exit_code(ExitStatus::from_raw(23 << 8)), 23);
    }

    #[test]
    fn represents_signal_status_using_the_shell_convention() {
        assert_eq!(supervisor_exit_code(ExitStatus::from_raw(6)), 134);
    }
}
