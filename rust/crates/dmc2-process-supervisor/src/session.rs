use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::os::raw::{c_int, c_ulong};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant, SystemTime, SystemTimeError, UNIX_EPOCH};

use crate::catalog::{self, Ownership, ProcessRole};
use crate::cli::{CliError, Invocation};
use crate::event::{encode_arguments, hex_bytes, Event};
use crate::journal::{Journal, JournalError};
use crate::limits::CoreDumpPlan;
use crate::process;
use crate::runtime::TRACKING_FAILURE_EXIT_CODE;
use crate::wait::{self, AnyWait, TerminalObservation, WaitEvidence};

const OBSERVATION_PERIOD: Duration = Duration::from_millis(5);

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
    let event = session_event(
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
    let mut children = BTreeMap::new();
    let root = ObservedChild {
        role: invocation.role.name().to_owned(),
        catalog_role: Some(invocation.role),
        relation: "direct-session-child",
        layer: "session-root",
        identity_source: "launcher-role",
        first_observed: observed_at,
    };
    let event = child_started_event(supervisor_pid, linuxcnc_pid, &root);
    let mut retained_journal_errors = Vec::new();
    append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
    children.insert(linuxcnc_pid, root);

    let mut linuxcnc_status = None;
    let mut children_probe_error_reported = false;
    let mut wait_error_reported = false;
    loop {
        match direct_children(supervisor_pid) {
            Ok(pids) => {
                if children_probe_error_reported {
                    let event = session_event(
                        "session-children-probe-restored",
                        unix_ns_or_zero(),
                        supervisor_pid,
                    );
                    append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                }
                children_probe_error_reported = false;
                for pid in pids {
                    if let Some(existing) = children.get(&pid) {
                        let mut observed = classify_child(pid);
                        if existing.should_reclassify_as(&observed) {
                            let event =
                                child_reclassified_event(supervisor_pid, pid, existing, &observed);
                            observed.first_observed = existing.first_observed;
                            append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                            children.insert(pid, observed);
                        }
                        continue;
                    }
                    let child = classify_child(pid);
                    let event = child_started_event(supervisor_pid, pid, &child);
                    append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                    children.insert(pid, child);
                }
            }
            Err(error) if !children_probe_error_reported => {
                children_probe_error_reported = true;
                let event = session_event(
                    "session-children-probe-failed",
                    unix_ns_or_zero(),
                    supervisor_pid,
                )
                .field("error_kind", format!("{:?}", error.kind()))
                .field("raw_os_error", optional_i32(error.raw_os_error()))
                .field("error_hex", hex_bytes(error.to_string().as_bytes()));
                append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
            }
            Err(_) => {}
        }

        let mut no_children = false;
        loop {
            let wait_result = wait::observe_any_nonblocking();
            if wait_error_reported && wait_result.is_ok() {
                let event =
                    session_event("session-wait-restored", unix_ns_or_zero(), supervisor_pid);
                append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                wait_error_reported = false;
            }
            match wait_result {
                Ok(AnyWait::Terminal(observation)) => {
                    let terminal_identity = classify_child(observation.pid);
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
                    let child = children.get(&observation.pid).or(Some(&terminal_identity));
                    let terminal_event = child_terminal_observed_event(
                        supervisor_pid,
                        linuxcnc_pid,
                        &session_root,
                        child,
                        &terminal_identity,
                        observation_state,
                        observation,
                        &children,
                    );
                    append_after_spawn(&mut journal, &terminal_event, &mut retained_journal_errors);
                    let evidence = match wait::reap_pid(observation.pid) {
                        Ok(evidence) => evidence,
                        Err(source) => {
                            let event = observation.event_fields(
                                session_event(
                                    "session-child-reap-failed",
                                    unix_ns_or_zero(),
                                    supervisor_pid,
                                )
                                .field("child_pid", observation.pid)
                                .field("error_kind", format!("{:?}", source.kind()))
                                .field("raw_os_error", optional_i32(source.raw_os_error()))
                                .field("error_hex", hex_bytes(source.to_string().as_bytes())),
                            );
                            append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                            return Err(SessionError::Reap {
                                child_pid: observation.pid,
                                source,
                            });
                        }
                    };
                    let child = children.remove(&evidence.pid).unwrap_or(terminal_identity);
                    let event = child_terminated_event(
                        supervisor_pid,
                        linuxcnc_pid,
                        &session_root,
                        Some(&child),
                        observation_state,
                        observation,
                        evidence,
                    );
                    append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                    if evidence.pid == linuxcnc_pid {
                        linuxcnc_status = Some(evidence.status);
                    }
                }
                Ok(AnyWait::Running) => break,
                Ok(AnyWait::NoChildren) => {
                    no_children = true;
                    break;
                }
                Err(error) if !wait_error_reported => {
                    wait_error_reported = true;
                    let event =
                        session_event("session-wait-failed", unix_ns_or_zero(), supervisor_pid)
                            .field("error_kind", format!("{:?}", error.kind()))
                            .field("raw_os_error", optional_i32(error.raw_os_error()))
                            .field("error_hex", hex_bytes(error.to_string().as_bytes()));
                    append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
                    break;
                }
                Err(_) => break,
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
            append_after_spawn(&mut journal, &event, &mut retained_journal_errors);
            if !retained_journal_errors.is_empty() {
                let first = retained_journal_errors.remove(0);
                return Err(SessionError::JournalAfterSpawn {
                    first,
                    additional_failures: retained_journal_errors.len(),
                });
            }
            return Ok(supervisor_exit_code(status));
        }

        thread::sleep(OBSERVATION_PERIOD);
    }
}

fn child_started_event(supervisor_pid: u32, pid: u32, child: &ObservedChild) -> Event {
    let event = session_event("session-child-observed", unix_ns_or_zero(), supervisor_pid)
        .field("role", &child.role)
        .field("relation", child.relation)
        .field("layer", child.layer)
        .field("identity_source", child.identity_source)
        .field("child_pid", pid);
    let event = catalog_event_fields(event, child.catalog_role);
    process::child_event_fields(event, pid)
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
        "elapsed_since_first_observed_ns",
        previous.first_observed.elapsed().as_nanos(),
    );
    let event = catalog_event_fields(event, observed.catalog_role);
    process::child_event_fields(event, pid)
}

fn child_terminal_observed_event(
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
    .field("observation_state", observation_state)
    .field("snapshot_phase", "terminal-before-reap");
    let event = session_root.event_fields(event, linuxcnc_pid);
    let event = catalog_event_fields(event, child.and_then(|child| child.catalog_role));
    let event = observation.event_fields(event);
    let event = tracked_children_event_fields(event, children, observation.pid, child);
    process::terminal_child_event_fields(event, observation.pid)
}

fn child_terminated_event(
    supervisor_pid: u32,
    linuxcnc_pid: u32,
    session_root: &SessionRootObservation,
    child: Option<&ObservedChild>,
    observation_state: &'static str,
    observation: TerminalObservation,
    evidence: WaitEvidence,
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
        unix_ns_or_zero(),
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
    .field("elapsed_since_observed_ns", observed_ns)
    .field("waitid_wait4_consistent", observation.agrees_with(evidence));
    let event = session_root.event_fields(event, linuxcnc_pid);
    let event = catalog_event_fields(event, child.and_then(|child| child.catalog_role));
    let event = observation.event_fields(event);
    let outcome = evidence.kernel_outcome();
    evidence.event_fields(event).field("outcome", outcome)
}

#[derive(Debug)]
enum SessionRootObservation {
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
    fn capture(terminal_pid: u32, linuxcnc_pid: u32, status_captured: bool) -> Self {
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

    fn event_fields(&self, event: Event, linuxcnc_pid: u32) -> Event {
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

fn classify_child(pid: u32) -> ObservedChild {
    let root = PathBuf::from(format!("/proc/{pid}"));
    let executable = fs::read_link(root.join("exe"));
    let cmdline = fs::read(root.join("cmdline")).unwrap_or_default();
    let comm = fs::read(root.join("comm")).ok();
    if let Some(role) = comm
        .as_deref()
        .and_then(|comm| catalog::identify_process_owner(comm).ok().flatten())
    {
        return ObservedChild::classified(
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
                name,
                role,
                "direct-process-owner",
                "supervisor-command-line-role",
            ),
            None => ObservedChild::unknown(
                "process-supervisor:unknown",
                "direct-process-owner",
                "unmatched",
            ),
        };
    }
    match catalog::identify_process(executable.as_deref().ok(), &cmdline, comm.as_deref()) {
        Ok(Some((role, source))) => {
            ObservedChild::classified(role.name(), role, "catalogued-workload", source.name())
        }
        Ok(None) | Err(_) => match basename {
            Some(name) => ObservedChild::unknown(
                format!("uncatalogued:{name}"),
                "uncatalogued-descendant",
                "proc-executable-basename-only",
            ),
            None => ObservedChild::unknown("unknown", "unidentified-descendant", "unmatched"),
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

fn append_after_spawn(journal: &mut Journal, event: &Event, failures: &mut Vec<JournalError>) {
    if let Err(error) = journal.append(event) {
        eprintln!(
            "dmc2-session-supervisor: lifecycle_journal_append=failed error={error} fallback_event={}",
            event.render()
        );
        failures.push(error);
    }
}

fn first_source(error: &SessionError) -> Option<&(dyn std::error::Error + 'static)> {
    match error {
        SessionError::Cli(error) => Some(error),
        SessionError::Journal(error) => Some(error),
        SessionError::JournalAfterSpawn { first, .. } => Some(first),
        SessionError::EnableSubreaper(error)
        | SessionError::VerifySubreaper(error)
        | SessionError::CoreDumpLimit { source: error, .. }
        | SessionError::OwnerIdentity { source: error, .. }
        | SessionError::Reap { source: error, .. }
        | SessionError::Spawn { source: error, .. }
        | SessionError::SpawnAndJournal { spawn: error, .. } => Some(error),
        SessionError::UnsupportedOwnership { .. } | SessionError::LinuxCncStatusMissing { .. } => {
            None
        }
        SessionError::CoreDumpLimitAndJournal { limit, .. } => Some(limit),
        SessionError::OwnerIdentityAndJournal { identity, .. } => Some(identity),
        SessionError::Clock(error) => Some(error),
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

struct ObservedChild {
    role: String,
    catalog_role: Option<ProcessRole>,
    relation: &'static str,
    layer: &'static str,
    identity_source: &'static str,
    first_observed: Instant,
}

impl ObservedChild {
    fn classified(
        name: impl Into<String>,
        role: ProcessRole,
        layer: &'static str,
        identity_source: &'static str,
    ) -> Self {
        Self {
            role: name.into(),
            catalog_role: Some(role),
            relation: "adopted-session-descendant",
            layer,
            identity_source,
            first_observed: Instant::now(),
        }
    }

    fn unknown(
        name: impl Into<String>,
        layer: &'static str,
        identity_source: &'static str,
    ) -> Self {
        Self {
            role: name.into(),
            catalog_role: None,
            relation: "adopted-session-descendant",
            layer,
            identity_source,
            first_observed: Instant::now(),
        }
    }

    fn should_reclassify_as(&self, observed: &Self) -> bool {
        self.catalog_role.is_none() && observed.catalog_role.is_some()
    }
}

#[derive(Debug)]
pub enum SessionError {
    Cli(CliError),
    UnsupportedOwnership {
        role: ProcessRole,
        ownership: Ownership,
    },
    Journal(JournalError),
    JournalAfterSpawn {
        first: JournalError,
        additional_failures: usize,
    },
    EnableSubreaper(io::Error),
    VerifySubreaper(io::Error),
    Clock(SystemTimeError),
    CoreDumpLimit {
        role: ProcessRole,
        source: io::Error,
    },
    CoreDumpLimitAndJournal {
        role: ProcessRole,
        limit: io::Error,
        journal: JournalError,
    },
    OwnerIdentity {
        role: ProcessRole,
        source: io::Error,
    },
    OwnerIdentityAndJournal {
        role: ProcessRole,
        identity: io::Error,
        journal: JournalError,
    },
    Spawn {
        role: ProcessRole,
        program: OsString,
        source: io::Error,
    },
    SpawnAndJournal {
        role: ProcessRole,
        program: OsString,
        spawn: io::Error,
        journal: JournalError,
    },
    LinuxCncStatusMissing {
        linuxcnc_pid: u32,
    },
    Reap {
        child_pid: u32,
        source: io::Error,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(error) => write!(formatter, "invalid invocation: {error}"),
            Self::UnsupportedOwnership { role, ownership } => write!(
                formatter,
                "role {} has ownership contract {}, not session-root",
                role.name(),
                ownership.name()
            ),
            Self::Journal(error) => write!(formatter, "lifecycle journal unavailable: {error}"),
            Self::JournalAfterSpawn {
                first,
                additional_failures,
            } => write!(
                formatter,
                "the session ran but {} lifecycle record(s) could not be persisted; first error: {first}",
                additional_failures + 1
            ),
            Self::EnableSubreaper(error) => {
                write!(
                    formatter,
                    "could not enable Linux child-subreaper ownership: {error}"
                )
            }
            Self::VerifySubreaper(error) => {
                write!(
                    formatter,
                    "could not verify Linux child-subreaper ownership: {error}"
                )
            }
            Self::Clock(error) => write!(formatter, "system clock predates Unix epoch: {error}"),
            Self::CoreDumpLimit { role, source } => write!(
                formatter,
                "could not establish the core-dump capture plan for role {}: {source}",
                role.name()
            ),
            Self::CoreDumpLimitAndJournal {
                role,
                limit,
                journal,
            } => write!(
                formatter,
                "could not establish the core-dump capture plan for role {}: {limit}; the lifecycle failure record also failed: {journal}",
                role.name()
            ),
            Self::OwnerIdentity { role, source } => write!(
                formatter,
                "could not establish the durable session-owner identity for role {}: {source}",
                role.name()
            ),
            Self::OwnerIdentityAndJournal {
                role,
                identity,
                journal,
            } => write!(
                formatter,
                "could not establish the durable session-owner identity for role {}: {identity}; the lifecycle failure record also failed: {journal}",
                role.name()
            ),
            Self::Spawn {
                role,
                program,
                source,
            } => write!(
                formatter,
                "could not spawn role {} program {program:?}: {source}",
                role.name()
            ),
            Self::SpawnAndJournal {
                role,
                program,
                spawn,
                journal,
            } => write!(
                formatter,
                "could not spawn role {} program {program:?}: {spawn}; the spawn-failure lifecycle record also failed: {journal}",
                role.name()
            ),
            Self::LinuxCncStatusMissing { linuxcnc_pid } => write!(
                formatter,
                "no children remain but LinuxCNC PID {linuxcnc_pid} had no captured wait status"
            ),
            Self::Reap { child_pid, source } => write!(
                formatter,
                "could not reap session child PID {child_pid} after terminal observation: {source}"
            ),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        first_source(self)
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
