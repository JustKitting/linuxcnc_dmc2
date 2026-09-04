mod failure;

use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant, SystemTime, SystemTimeError, UNIX_EPOCH};

use crate::backtrace::{self, BacktraceEvidence};
use crate::catalog::{BacktraceKind, Ownership, ProcessRole};
use crate::cli::Invocation;
use crate::event::{encode_arguments, Event};
use crate::journal::{FailureTracker, Journal};
use crate::limits::CoreDumpPlan;
use crate::{process, wait};

use failure::StartupStage;
pub use failure::SupervisorError;

pub const TRACKING_FAILURE_EXIT_CODE: u8 = 125;
const WAIT_RETRY_DELAY: Duration = Duration::from_millis(10);

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
        let event = base_event("owner-identity-failed", supervisor_pid, invocation.role)
            .field("error", source.to_string());
        let journal_error = journal.append(&event).err();
        return Err(SupervisorError::startup(
            StartupStage::OwnerIdentity,
            invocation.role,
            invocation.program,
            source,
            journal_error,
        ));
    }
    let core_dump_plan = match CoreDumpPlan::capture(invocation.role.core_dump_policy()) {
        Ok(plan) => plan,
        Err(source) => {
            let event = base_event("core-limit-plan-failed", supervisor_pid, invocation.role)
                .field("error", source.to_string());
            let journal_error = journal.append(&event).err();
            return Err(SupervisorError::startup(
                StartupStage::CoreDumpLimit,
                invocation.role,
                invocation.program,
                source,
                journal_error,
            ));
        }
    };
    let started_ns = unix_ns().map_err(SupervisorError::Clock)?;
    let event = base_event_at(
        "supervisor-started",
        started_ns,
        supervisor_pid,
        invocation.role,
    )
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
    journal.append(&event).map_err(SupervisorError::Journal)?;

    let launched_at_wall = SystemTime::now();
    let launched_at = Instant::now();
    let mut command = Command::new(&invocation.program);
    command.args(&invocation.arguments);
    core_dump_plan.configure(&mut command);
    let child = match command.spawn() {
        Ok(child) => child,
        Err(source) => {
            let event = base_event("spawn-failed", supervisor_pid, invocation.role)
                .encoded_os_field("program_hex", &invocation.program)
                .field("error", source.to_string());
            let journal_error = journal.append(&event).err();
            return Err(SupervisorError::startup(
                StartupStage::Spawn,
                invocation.role,
                invocation.program,
                source,
                journal_error,
            ));
        }
    };
    let child_pid = child.id();
    let mut journal_failures = FailureTracker::new();
    let event = process::event_fields(
        base_event("process-started", supervisor_pid, invocation.role)
            .encoded_os_field("program_hex", &invocation.program)
            .field("argc", invocation.arguments.len())
            .field("argv_hex", encode_arguments(&invocation.arguments)),
        child_pid,
    );
    append_after_spawn(&mut journal, &event, invocation.role, &mut journal_failures);

    let mut wait_failures = 0_u64;
    let evidence = loop {
        match wait::wait_pid(child_pid) {
            Ok(evidence) => {
                if wait_failures != 0 {
                    let event =
                        base_event("process-wait-recovered", supervisor_pid, invocation.role)
                            .field("child_pid", child_pid)
                            .field("wait_failures", wait_failures);
                    append_after_spawn(
                        &mut journal,
                        &event,
                        invocation.role,
                        &mut journal_failures,
                    );
                    eprintln!("dmc2-process-supervisor: role={} child_pid={child_pid} wait4 recovered after {wait_failures} failure(s)", invocation.role.name());
                }
                break evidence;
            }
            Err(source) => {
                wait_failures = wait_failures.saturating_add(1);
                if wait_failures == 1 {
                    let event = base_event("process-wait-failed", supervisor_pid, invocation.role)
                        .field("child_pid", child_pid)
                        .field("error", source.to_string())
                        .field("recovery", "retry-exact-child-without-releasing-ownership");
                    append_after_spawn(
                        &mut journal,
                        &event,
                        invocation.role,
                        &mut journal_failures,
                    );
                    eprintln!("dmc2-process-supervisor: role={} child_pid={child_pid} wait4 failed: {source}; retaining ownership and retrying", invocation.role.name());
                }
                match process::exists(child_pid) {
                    Ok(false) => {
                        return Err(SupervisorError::WaitStatusLost {
                            role: invocation.role,
                            child_pid,
                            source,
                            journal_failures: journal_failures.failures(),
                        })
                    }
                    Ok(true) => {}
                    Err(presence_error) if wait_failures == 1 => {
                        let event = base_event(
                            "process-presence-check-failed",
                            supervisor_pid,
                            invocation.role,
                        )
                        .field("child_pid", child_pid)
                        .field("error", presence_error.to_string())
                        .field("recovery", "retain-owner-and-retry");
                        append_after_spawn(
                            &mut journal,
                            &event,
                            invocation.role,
                            &mut journal_failures,
                        );
                    }
                    Err(_) => {}
                }
                thread::sleep(WAIT_RETRY_DELAY);
            }
        }
    };

    let exit_ns = unix_ns_or_zero();
    let backtrace = match invocation.role.backtrace() {
        BacktraceKind::None => BacktraceEvidence::NotApplicable,
        BacktraceKind::LinuxCncTask => {
            backtrace::capture(journal.path(), child_pid, launched_at_wall, exit_ns)
        }
    };
    let outcome = if backtrace.reported_signal().is_some() {
        "linuxcnc-handled-fatal-signal"
    } else if evidence.status.signal().is_some() {
        "kernel-signal-termination"
    } else if evidence.status.success() {
        "zero-exit"
    } else {
        "nonzero-exit"
    };
    let event = base_event_at(
        "process-terminated",
        exit_ns,
        supervisor_pid,
        invocation.role,
    )
    .field("elapsed_ns", launched_at.elapsed().as_nanos())
    .field("outcome", outcome);
    let event = evidence.event_fields(event);
    let event = backtrace.event_fields(event);
    let event = journal_failures.event_fields(event);
    append_after_spawn(&mut journal, &event, invocation.role, &mut journal_failures);
    if let Some(first) = journal_failures.take_first() {
        return Err(SupervisorError::JournalAfterChildSpawn {
            first,
            additional_failures: journal_failures.additional_failures(),
        });
    }
    Ok(supervisor_exit_code(evidence.status))
}

fn append_after_spawn(
    journal: &mut Journal,
    event: &Event,
    role: ProcessRole,
    failures: &mut FailureTracker,
) {
    match journal.append(event) {
        Ok(()) => {
            if failures.record_success() {
                eprintln!(
                    "dmc2-process-supervisor: role={} lifecycle journal recovered",
                    role.name()
                );
            }
        }
        Err(error) => {
            eprintln!("dmc2-process-supervisor: role={} lifecycle journal failed: {error}; recovery: writing will be retried while the child remains owned", role.name());
            failures.record_failure(error);
        }
    }
}

fn base_event(kind: &'static str, supervisor_pid: u32, role: ProcessRole) -> Event {
    base_event_at(kind, unix_ns_or_zero(), supervisor_pid, role)
}

fn base_event_at(
    kind: &'static str,
    unix_ns: u128,
    supervisor_pid: u32,
    role: ProcessRole,
) -> Event {
    Event::new(kind, unix_ns, supervisor_pid)
        .field("role", role.name())
        .field("launch_site", role.launch_site())
        .field("ownership", role.ownership().name())
        .field("criticality", role.criticality().name())
        .field("backtrace_contract", role.backtrace().name())
        .field("core_dump_policy", role.core_dump_policy().name())
        .field("owner_comm", role.owner_comm())
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

fn unix_ns() -> Result<u128, SystemTimeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
}

fn unix_ns_or_zero() -> u128 {
    unix_ns().unwrap_or(0)
}
