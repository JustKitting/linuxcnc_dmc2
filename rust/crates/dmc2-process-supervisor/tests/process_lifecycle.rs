use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use dmc2_process_supervisor::Invocation;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "dmc2-process-supervisor-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated test directory");
        Self(path)
    }

    fn journal(&self) -> PathBuf {
        self.0.join("lifecycle.tsv")
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn records_the_kernel_exit_code_and_rusage_from_a_real_child() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let status = run_supervisor(&journal, "exit 23");

    assert_eq!(status.code(), Some(23));
    let records = fs::read_to_string(&journal).expect("read lifecycle journal");
    assert_complete_records(&records);
    assert!(records.contains("\tevent=process-started\t"));
    assert!(records.contains("\tevent=process-terminal-observed\t"));
    assert!(records.contains("\tevent=process-terminated\t"));
    assert!(records.contains("\trole=lifecycle-test\t"));
    assert!(records.contains("\texit_code=23\t"));
    assert!(records.contains("\tsignal=NONE\t"));
    assert!(records.contains("\tproc_parent_pid="));
    assert!(records.contains("\tproc_start_time_ticks="));
    assert!(records.contains("\tproc_state=Z\t"));
    assert!(records.contains("\tsnapshot_phase=terminal-before-reap\t"));
    assert!(records.contains("\twaitid_code_name=CLD_EXITED\t"));
    assert!(records.contains("\twaitid_uid="));
    assert!(records.contains("\tcore_limit_plan_policy=enable-to-hard-limit\t"));
    assert!(records.contains("\thost_core_pattern_hex="));
    assert!(records.contains("\tcgroup_snapshot_state=captured\t"));
    assert!(records.contains("\tcgroup_memory_events_hex="));
    assert!(records.contains("\tcgroup_pids_events_hex="));
    assert!(records.contains("\trusage_max_resident_kib="));
    assert!(records.contains("\twaitid_wait4_consistent=true\t"));
    assert!(records.contains("\toutcome=nonzero-exit\t"));
    let terminal = find_record(&records, "process-terminated", |_| true);
    assert_eq!(
        journal_field(terminal, "journal_failures_after_spawn"),
        Some("0")
    );
    assert_eq!(
        journal_field(terminal, "journal_failure_recoveries"),
        Some("0")
    );
    assert_eq!(
        journal_field(terminal, "journal_failure_active"),
        Some("false")
    );
}

#[test]
fn records_the_kernel_signal_from_a_real_child() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let status = run_supervisor(&journal, "ulimit -c 0; kill -ABRT $$");

    assert_eq!(status.code(), Some(134));
    let records = fs::read_to_string(&journal).expect("read lifecycle journal");
    assert_complete_records(&records);
    assert!(records.contains("\texit_code=NONE\t"));
    assert!(records.contains("\tsignal=6\t"));
    assert!(records.contains("\tsignal_name=SIGABRT\t"));
    assert!(records.contains("\tcore_dumped=false\t"));
    assert!(records.contains("\tcore_artifact_state=not-dumped\t"));
    assert!(records.contains("\toutcome=kernel-signal-termination\t"));
}

#[test]
fn waitid_failure_keeps_the_real_child_owned_until_wait4_reaps_its_status() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let release = directory.0.join("release-waitid-fallback-child");
    let interposer = directory.0.join("libdmc2_wait_fault.so");
    compile_wait_fault_fixture(&interposer);
    let script = format!("while [ ! -e {} ]; do :; done; exit 37", release.display());
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_dmc2-process-supervisor"))
        .env("LD_PRELOAD", &interposer)
        .env("DMC2_WAIT_FAULT_MODE", "waitid-then-fallback-wait4")
        .arg("--role")
        .arg("lifecycle-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script)
        .spawn()
        .expect("spawn waitid-failure supervisor");

    assert!(
        wait_for_journal_record(&journal, |line| {
            journal_field(line, "event") == Some("terminal-observe-failed")
                && journal_field(line, "raw_os_error") == Some("1")
                && journal_field(line, "child_ownership_released") == Some("false")
                && journal_field(line, "terminal_acquisition_fallback")
                    == Some("wait4-wnohang-reaping")
        }),
        "supervisor did not record the forced waitid failure"
    );
    assert!(
        wait_for_journal_record(&journal, |line| {
            journal_field(line, "event") == Some("terminal-fallback-reap-poll-failed")
                && journal_field(line, "fallback_wait4_raw_os_error") == Some("1")
                && journal_field(line, "child_ownership_released") == Some("false")
        }),
        "supervisor did not retain the forced fallback wait4 failure"
    );
    let child_pid = wait_for_journal_field(&journal, "process-started", "child_pid")
        .and_then(|value| value.parse::<u32>().ok())
        .expect("supervisor recorded its real child PID");
    assert!(
        PathBuf::from(format!("/proc/{child_pid}")).is_dir(),
        "supervisor abandoned the still-running child after waitid failed"
    );

    fs::write(&release, b"release").expect("release waitid-fallback child");
    let status = supervisor.wait().expect("wait for degraded supervisor");

    assert_eq!(status.code(), Some(125));
    let records = fs::read_to_string(&journal).expect("read waitid-fallback journal");
    assert_complete_records(&records);
    assert!(!records.contains("\tevent=process-terminal-observed\t"));
    let fallback = find_record(&records, "process-terminal-reaped-fallback", |_| true);
    assert_eq!(journal_field(fallback, "exit_code"), Some("37"));
    assert_eq!(
        journal_field(fallback, "terminal_acquisition_method"),
        Some("wait4-wnohang-after-waitid-error")
    );
    assert_eq!(
        journal_field(fallback, "terminal_proc_snapshot_state"),
        Some("not-attempted-after-reap-to-avoid-pid-reuse")
    );
    let terminal = find_record(&records, "process-terminated", |_| true);
    assert_eq!(journal_field(terminal, "exit_code"), Some("37"));
    assert_eq!(
        journal_field(terminal, "terminal_observation_degraded"),
        Some("true")
    );
    assert_eq!(journal_field(terminal, "waitid_available"), Some("false"));
    assert_eq!(
        journal_field(terminal, "waitid_wait4_consistent"),
        Some("NOT_COMPARABLE")
    );
    assert_eq!(
        journal_field(terminal, "fallback_wait4_failures"),
        Some("3")
    );
    assert_eq!(
        journal_field(terminal, "fallback_wait4_recoveries"),
        Some("1")
    );
    assert_eq!(
        records
            .lines()
            .filter(|line| {
                journal_field(line, "event") == Some("terminal-fallback-reap-poll-failed")
            })
            .count(),
        1,
        "identical fallback failures should be counted without journal flooding"
    );
    assert!(records.contains("\tevent=terminal-fallback-reap-poll-restored\t"));
    assert!(
        fs::metadata(format!("/proc/{child_pid}")).is_err(),
        "fallback wait4 did not reap the child"
    );
}

#[test]
fn session_waitid_failure_keeps_the_real_root_owned_until_wait4_reaps_it() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let release = directory.0.join("release-session-waitid-fallback-root");
    let interposer = directory.0.join("libdmc2_wait_fault.so");
    compile_wait_fault_fixture(&interposer);
    let script = format!("while [ ! -e {} ]; do :; done; exit 43", release.display());
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_dmc2-session-supervisor"))
        .env("LD_PRELOAD", &interposer)
        .env("DMC2_WAIT_FAULT_MODE", "waitid-then-fallback-wait4")
        .arg("--role")
        .arg("session-lifecycle-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script)
        .spawn()
        .expect("spawn session waitid-failure supervisor");

    assert!(
        wait_for_journal_record(&journal, |line| {
            journal_field(line, "event") == Some("session-wait-failed")
                && journal_field(line, "raw_os_error") == Some("1")
                && journal_field(line, "session_ownership_released") == Some("false")
                && journal_field(line, "terminal_acquisition_fallback")
                    == Some("wait4-p-all-wnohang-reaping")
        }),
        "session supervisor did not record the forced waitid failure"
    );
    assert!(
        wait_for_journal_record(&journal, |line| {
            journal_field(line, "event") == Some("session-fallback-reap-poll-failed")
                && journal_field(line, "fallback_wait4_raw_os_error") == Some("1")
                && journal_field(line, "session_ownership_released") == Some("false")
        }),
        "session supervisor did not retain the forced fallback wait4 failure"
    );
    let root_pid = wait_for_journal_field_for_role(
        &journal,
        "session-child-observed",
        "session-lifecycle-test",
        "child_pid",
    )
    .and_then(|value| value.parse::<u32>().ok())
    .expect("session supervisor recorded its real root PID");
    assert!(
        PathBuf::from(format!("/proc/{root_pid}")).is_dir(),
        "session supervisor abandoned the still-running root after waitid failed"
    );

    fs::write(&release, b"release").expect("release session waitid-fallback root");
    let status = supervisor
        .wait()
        .expect("wait for degraded session supervisor");

    assert_eq!(status.code(), Some(125));
    let records = fs::read_to_string(&journal).expect("read session waitid-fallback journal");
    assert_complete_records(&records);
    assert!(!records.contains("\tevent=session-child-terminal-observed\t"));
    let fallback = find_record(&records, "session-child-terminal-reaped-fallback", |_| true);
    assert_eq!(journal_field(fallback, "exit_code"), Some("43"));
    assert_eq!(
        journal_field(fallback, "terminal_acquisition_method"),
        Some("wait4-p-all-wnohang-after-waitid-error")
    );
    assert_eq!(
        journal_field(fallback, "terminal_proc_snapshot_state"),
        Some("not-attempted-after-reap-to-avoid-pid-reuse")
    );
    let terminal = find_record(&records, "session-child-terminated", |_| true);
    assert_eq!(journal_field(terminal, "exit_code"), Some("43"));
    assert_eq!(journal_field(terminal, "waitid_available"), Some("false"));
    assert_eq!(
        journal_field(terminal, "terminal_observation_degraded"),
        Some("true")
    );
    assert_eq!(
        journal_field(terminal, "fallback_wait4_failures"),
        Some("3")
    );
    assert_eq!(
        journal_field(terminal, "fallback_wait4_recoveries"),
        Some("1")
    );
    let session_terminal = find_record(&records, "session-supervisor-terminated", |_| true);
    assert_eq!(
        journal_field(session_terminal, "linuxcnc_exit_code"),
        Some("43")
    );
    assert_eq!(
        journal_field(session_terminal, "terminal_observation_degraded"),
        Some("true")
    );
    assert_eq!(
        records
            .lines()
            .filter(|line| {
                journal_field(line, "event") == Some("session-fallback-reap-poll-failed")
            })
            .count(),
        1,
        "identical session fallback failures should not flood the journal"
    );
    assert!(records.contains("\tevent=session-fallback-reap-poll-restored\t"));
    assert!(
        fs::metadata(format!("/proc/{root_pid}")).is_err(),
        "session fallback wait4 did not reap the root"
    );
}

#[test]
fn retained_terminal_wait4_failure_does_not_release_the_direct_child() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let release = directory.0.join("release-retained-direct-reap");
    let interposer = directory.0.join("libdmc2_wait_fault.so");
    compile_wait_fault_fixture(&interposer);
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_dmc2-process-supervisor"))
        .env("LD_PRELOAD", &interposer)
        .env("DMC2_WAIT_FAULT_MODE", "retained-terminal-wait4")
        .env("DMC2_WAIT_FAULT_RELEASE", &release)
        .arg("--role")
        .arg("lifecycle-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg("exit 47")
        .spawn()
        .expect("spawn retained-reap-failure supervisor");

    assert!(
        wait_for_journal_record(&journal, |line| {
            journal_field(line, "event") == Some("retained-terminal-reap-failed")
                && journal_field(line, "reap_wait4_raw_os_error") == Some("1")
                && journal_field(line, "child_ownership_released") == Some("false")
                && journal_field(line, "terminal_waitid_status_retained") == Some("true")
        }),
        "direct owner did not record the retained-terminal wait4 failure"
    );
    let child_pid = wait_for_journal_field(&journal, "process-started", "child_pid")
        .and_then(|value| value.parse::<u32>().ok())
        .expect("direct owner recorded its real child PID");
    assert!(
        PathBuf::from(format!("/proc/{child_pid}")).is_dir(),
        "direct owner released the terminal child while wait4 was failing"
    );
    assert!(
        supervisor
            .try_wait()
            .expect("inspect direct owner while reap is unavailable")
            .is_none(),
        "direct owner exited before retaining the child's real wait status"
    );

    fs::write(&release, b"release").expect("release retained direct reap");
    let status = supervisor.wait().expect("wait for retained direct owner");
    assert_eq!(status.code(), Some(125));

    let records = fs::read_to_string(&journal).expect("read retained direct reap journal");
    assert_complete_records(&records);
    let terminal = find_record(&records, "process-terminated", |_| true);
    assert_eq!(journal_field(terminal, "exit_code"), Some("47"));
    assert_eq!(journal_field(terminal, "waitid_available"), Some("true"));
    assert_eq!(
        journal_field(terminal, "waitid_wait4_consistent"),
        Some("true")
    );
    assert_eq!(
        journal_field(terminal, "terminal_reap_degraded"),
        Some("true")
    );
    assert!(
        journal_field(terminal, "reap_wait4_failures")
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|failures| failures > 0),
        "terminal record did not retain the wait4 failure count"
    );
    assert_eq!(
        records
            .lines()
            .filter(|line| {
                journal_field(line, "event") == Some("retained-terminal-reap-failed")
            })
            .count(),
        1,
        "identical retained-reap failures should not flood the journal"
    );
    assert!(records.contains("\tevent=retained-terminal-reap-restored\t"));
    assert!(
        fs::metadata(format!("/proc/{child_pid}")).is_err(),
        "direct owner did not reap the exact child after wait4 recovered"
    );
}

#[test]
fn retained_terminal_wait4_failure_does_not_release_the_session_child() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let release = directory.0.join("release-retained-session-reap");
    let interposer = directory.0.join("libdmc2_wait_fault.so");
    compile_wait_fault_fixture(&interposer);
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_dmc2-session-supervisor"))
        .env("LD_PRELOAD", &interposer)
        .env("DMC2_WAIT_FAULT_MODE", "retained-terminal-wait4")
        .env("DMC2_WAIT_FAULT_RELEASE", &release)
        .arg("--role")
        .arg("session-lifecycle-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg("exit 53")
        .spawn()
        .expect("spawn retained session-reap-failure supervisor");

    assert!(
        wait_for_journal_record(&journal, |line| {
            journal_field(line, "event") == Some("session-retained-terminal-reap-failed")
                && journal_field(line, "reap_wait4_raw_os_error") == Some("1")
                && journal_field(line, "child_ownership_released") == Some("false")
                && journal_field(line, "terminal_waitid_status_retained") == Some("true")
        }),
        "session owner did not record the retained-terminal wait4 failure"
    );
    let root_pid = wait_for_journal_field_for_role(
        &journal,
        "session-child-observed",
        "session-lifecycle-test",
        "child_pid",
    )
    .and_then(|value| value.parse::<u32>().ok())
    .expect("session owner recorded its real root PID");
    assert!(
        PathBuf::from(format!("/proc/{root_pid}")).is_dir(),
        "session owner released the terminal root while wait4 was failing"
    );
    assert!(
        supervisor
            .try_wait()
            .expect("inspect session owner while reap is unavailable")
            .is_none(),
        "session owner exited before retaining the root's real wait status"
    );

    fs::write(&release, b"release").expect("release retained session reap");
    let status = supervisor.wait().expect("wait for retained session owner");
    assert_eq!(status.code(), Some(125));

    let records = fs::read_to_string(&journal).expect("read retained session reap journal");
    assert_complete_records(&records);
    let terminal = find_record(&records, "session-child-terminated", |_| true);
    assert_eq!(journal_field(terminal, "exit_code"), Some("53"));
    assert_eq!(journal_field(terminal, "waitid_available"), Some("true"));
    assert_eq!(
        journal_field(terminal, "waitid_wait4_consistent"),
        Some("true")
    );
    assert_eq!(
        journal_field(terminal, "terminal_reap_degraded"),
        Some("true")
    );
    let session_terminal = find_record(&records, "session-supervisor-terminated", |_| true);
    assert_eq!(
        journal_field(session_terminal, "terminal_reap_degraded"),
        Some("true")
    );
    assert_eq!(
        journal_field(session_terminal, "terminal_reap_degraded_children"),
        Some("1")
    );
    assert!(
        journal_field(session_terminal, "terminal_reap_wait4_failures")
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|failures| failures > 0),
        "session terminal record did not retain the wait4 failure count"
    );
    assert_eq!(
        records
            .lines()
            .filter(|line| {
                journal_field(line, "event") == Some("session-retained-terminal-reap-failed")
            })
            .count(),
        1,
        "identical session retained-reap failures should not flood the journal"
    );
    assert!(records.contains("\tevent=session-retained-terminal-reap-restored\t"));
    assert!(
        fs::metadata(format!("/proc/{root_pid}")).is_err(),
        "session owner did not reap the exact child after wait4 recovered"
    );
}

#[test]
fn records_the_sender_of_a_real_caught_sigterm_before_the_clean_exit() {
    assert_real_caught_signal("-TERM", 15, "SIGTERM");
}

#[test]
fn records_the_sender_of_a_real_caught_sigint_before_the_clean_exit() {
    assert_real_caught_signal("-INT", 2, "SIGINT");
}

#[test]
fn distinguishes_a_real_armed_zero_exit_without_a_caught_signal() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let ready = directory.0.join("signal-handler-ready");
    let library = directory.0.join("libdmc2_signal_evidence.so");
    let fixture = directory.0.join("dmc2-signal-fixture");
    compile_signal_evidence_fixture(&library, &fixture);

    let status = Command::new(env!("CARGO_BIN_EXE_dmc2-process-supervisor"))
        .current_dir(&directory.0)
        .env("DMC2_SIGNAL_EVIDENCE_TEST_LIBRARY", &library)
        .arg("--role")
        .arg("signal-evidence-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("./dmc2-signal-fixture")
        .arg(&ready)
        .arg("exit-clean")
        .status()
        .expect("run armed zero-exit fixture");

    assert_eq!(status.code(), Some(0));
    let records = fs::read_to_string(&journal).expect("read armed zero-exit journal");
    assert_complete_records(&records);
    assert!(!records.contains("\tcaught_signal_record_kind=signal-delivered\t"));
    let terminal = find_record(&records, "process-terminated", |_| true);
    assert_eq!(
        journal_field(terminal, "caught_signal_evidence_state"),
        Some("ready")
    );
    assert_eq!(
        journal_field(terminal, "caught_signal_matching_delivered_records"),
        Some("0")
    );
    assert_eq!(
        journal_field(terminal, "outcome"),
        Some("zero-exit-without-caught-signal-evidence")
    );
}

#[test]
fn a_dead_evidence_owner_cannot_sigpipe_its_adopted_workload() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let ready = directory.0.join("orphan-signal-handler-ready");
    let owner_pid_file = directory.0.join("signal-owner-pid");
    let library = directory.0.join("libdmc2_signal_evidence.so");
    let fixture = directory.0.join("dmc2-signal-fixture");
    compile_signal_evidence_fixture(&library, &fixture);
    let owner = env!("CARGO_BIN_EXE_dmc2-process-supervisor");
    let script = format!(
        "{owner} --role signal-evidence-test --journal {} -- ./dmc2-signal-fixture {} & printf '%s' \"$!\" > {}; exit 7",
        journal.display(),
        ready.display(),
        owner_pid_file.display(),
    );
    let mut session = Command::new(env!("CARGO_BIN_EXE_dmc2-session-supervisor"))
        .current_dir(&directory.0)
        .env("DMC2_SIGNAL_EVIDENCE_TEST_LIBRARY", &library)
        .arg("--role")
        .arg("session-lifecycle-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script)
        .spawn()
        .expect("spawn owner-loss signal-evidence session");
    assert!(wait_for_path(&ready), "signal fixture never became ready");
    assert!(
        wait_for_journal_record(&journal, |line| {
            journal_field(line, "event") == Some("caught-signal-evidence-record")
                && journal_field(line, "caught_signal_record_kind") == Some("handler-armed")
                && journal_field(line, "caught_signal") == Some("15")
        }),
        "direct owner never persisted the SIGTERM registration"
    );
    let target_pid = wait_for_journal_field_for_role(
        &journal,
        "process-started",
        "signal-evidence-test",
        "child_pid",
    )
    .and_then(|value| value.parse::<u32>().ok())
    .expect("direct owner recorded its fixture PID");
    let owner_pid = fs::read_to_string(&owner_pid_file)
        .expect("read direct owner PID")
        .parse::<u32>()
        .expect("direct owner PID is numeric");

    send_signal_and_wait("-KILL", owner_pid);
    assert!(
        wait_for_journal_record(&journal, |line| {
            journal_field(line, "event") == Some("session-child-terminated")
                && journal_field(line, "child_pid") == Some(owner_pid.to_string().as_str())
                && journal_field(line, "signal") == Some("9")
        }),
        "session subreaper did not retain the killed owner's terminal status"
    );
    send_signal_and_wait("-TERM", target_pid);
    let status = session.wait().expect("wait for owner-loss session");

    assert_eq!(status.code(), Some(7));
    let records = fs::read_to_string(&journal).expect("read owner-loss signal journal");
    assert_complete_records(&records);
    let target_pid_text = target_pid.to_string();
    let target_terminal = find_record(&records, "session-child-terminated", |line| {
        journal_field(line, "child_pid") == Some(target_pid_text.as_str())
    });
    assert_eq!(journal_field(target_terminal, "exit_code"), Some("0"));
    assert_eq!(journal_field(target_terminal, "signal"), Some("NONE"));
    assert_eq!(
        journal_field(target_terminal, "relation"),
        Some("adopted-session-descendant")
    );
}

#[test]
fn terminal_record_contains_a_rolling_pre_death_fd_snapshot() {
    use std::os::unix::ffi::OsStrExt;

    let directory = TestDirectory::new();
    let journal = directory.journal();
    let tracked_file = directory.0.join("opened-after-initial-snapshot");
    let release = directory.0.join("open-descriptor-now");
    fs::write(&tracked_file, b"snapshot target").expect("create tracked descriptor target");
    let script = format!(
        "attempt=0; while [ ! -e {} ] && [ \"$attempt\" -lt 400 ]; do attempt=$((attempt + 1)); sleep 0.005; done; [ -e {} ] || exit 99; exec 9<{}; sleep 0.05; exit 31",
        release.display(),
        release.display(),
        tracked_file.display()
    );
    let mut supervisor = spawn_supervisor(&journal, &script);
    assert!(
        wait_for_journal_record(&journal, |line| line.contains("\tevent=process-started\t")),
        "direct owner never committed its initial process observation"
    );
    fs::write(&release, b"release").expect("release direct snapshot child");
    let status = supervisor.wait().expect("wait for rolling-snapshot owner");

    assert_eq!(status.code(), Some(31));
    let records = fs::read_to_string(&journal).expect("read rolling-snapshot journal");
    assert_complete_records(&records);
    let terminal = records
        .lines()
        .find(|line| line.contains("\tevent=process-terminal-observed\t"))
        .expect("terminal-before-reap record");
    let expected_target = encode_hex(tracked_file.as_os_str().as_bytes());
    assert!(
        terminal.contains("\tlast_live_snapshot_state=captured\t"),
        "{terminal}"
    );
    assert!(
        terminal.contains("\tlive_snapshot_successes=")
            && terminal.contains("\tlast_live_fd_catalog_state=captured\t"),
        "{terminal}"
    );
    assert!(terminal.contains(&expected_target), "{terminal}");
}

#[test]
fn preserves_the_kernel_core_from_a_real_crashing_child() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let status = Command::new(env!("CARGO_BIN_EXE_dmc2-process-supervisor"))
        .current_dir(&directory.0)
        .arg("--role")
        .arg("lifecycle-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg("kill -SEGV $$")
        .status()
        .expect("run real kernel-core lifecycle test");

    assert_eq!(status.code(), Some(139));
    let records = fs::read_to_string(&journal).expect("read kernel-core lifecycle journal");
    assert_complete_records(&records);
    let terminal = records
        .lines()
        .find(|line| line.contains("\tevent=process-terminated\t"))
        .expect("kernel-core terminal record");
    assert!(terminal.contains("\tsignal=11\t"), "{terminal}");
    assert!(terminal.contains("\tcore_dumped=true\t"), "{terminal}");
    assert!(
        terminal.contains("\tcore_artifact_state=captured\t"),
        "{terminal}"
    );
    let copy = terminal
        .split('\t')
        .find_map(|field| field.strip_prefix("core_artifact_copy_hex="))
        .map(decode_hex_path)
        .expect("durable kernel-core copy path");
    let metadata = fs::metadata(copy).expect("durable kernel-core copy");
    assert!(metadata.is_file());
    assert!(metadata.len() > 0);
}

#[test]
fn preserves_a_linuxcnc_caught_fatal_signal_that_returns_zero() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let status = Command::new(env!("CARGO_BIN_EXE_dmc2-process-supervisor"))
        .arg("--role")
        .arg("task-backtrace-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg("sleep 0.02; printf 'pid=%s signal=11\\n' \"$$\" > \"/tmp/backtrace.$$\"; exit 0")
        .status()
        .expect("run LinuxCNC caught-signal lifecycle test");

    assert_eq!(status.code(), Some(0));
    let records = fs::read_to_string(&journal).expect("read lifecycle journal");
    let child_pid = records
        .lines()
        .find(|line| line.contains("\tevent=process-started\t"))
        .and_then(|line| {
            line.split('\t')
                .find_map(|field| field.strip_prefix("child_pid="))
        })
        .and_then(|value| value.parse::<u32>().ok())
        .expect("started child PID");
    let backtrace_source = PathBuf::from(format!("/tmp/backtrace.{child_pid}"));

    assert_complete_records(&records);
    assert!(records.contains("\trole=task-backtrace-test\t"));
    assert!(records.contains("\texit_code=0\t"));
    assert!(
        records.contains("\tbacktrace_state=captured\t"),
        "{records}"
    );
    assert!(records.contains("\tbacktrace_signal=11\t"));
    assert!(records.contains("\tbacktrace_signal_name=SIGSEGV\t"));
    assert!(records.contains("\toutcome=linuxcnc-handled-fatal-signal\t"));
    fs::remove_file(backtrace_source).expect("remove synthetic LinuxCNC backtrace");
}

#[test]
fn concurrent_supervisors_commit_noninterleaved_checked_records() {
    const CHILDREN: usize = 12;
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let mut children = (0..CHILDREN)
        .map(|_| spawn_supervisor(&journal, "exit 0"))
        .collect::<Vec<_>>();
    for child in &mut children {
        assert_eq!(child.wait().expect("wait for supervisor").code(), Some(0));
    }

    let records = fs::read_to_string(&journal).expect("read shared lifecycle journal");
    assert_complete_records(&records);
    for required_event in [
        "supervisor-started",
        "process-started",
        "process-terminal-observed",
        "process-terminated",
    ] {
        assert_eq!(
            records
                .lines()
                .filter(|line| journal_field(line, "event") == Some(required_event))
                .count(),
            CHILDREN,
            "missing or duplicated {required_event} records"
        );
    }
    for record in records.lines() {
        let event = journal_field(record, "event").expect("journal event field");
        assert!(
            matches!(
                event,
                "supervisor-started"
                    | "process-started"
                    | "live-snapshot-unavailable"
                    | "live-snapshot-restored"
                    | "process-terminal-observed"
                    | "process-terminated"
            ),
            "unexpected event in concurrent-writer journal: {event}: {record}"
        );
    }
}

#[test]
fn session_subreaper_records_the_root_and_an_adopted_descendant() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let status = Command::new(env!("CARGO_BIN_EXE_dmc2-session-supervisor"))
        .arg("--role")
        .arg("session-lifecycle-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg("(sleep 0.05; exit 42) & exit 7")
        .status()
        .expect("run session subreaper");

    assert_eq!(status.code(), Some(7));
    let records = fs::read_to_string(&journal).expect("read session lifecycle journal");
    assert_complete_records(&records);
    assert!(records.contains("\tevent=session-supervisor-started\t"));
    assert!(records.contains("\tevent=session-supervisor-terminated\t"));
    assert!(records.contains("\tevent=session-child-terminal-observed\t"));
    assert!(records.contains("\trole=session-lifecycle-test\t"));
    assert!(records.contains("\trelation=direct-session-child\t"));
    assert!(records.contains("\trelation=adopted-session-descendant\t"));
    assert!(records.contains("\texit_code=7\t"));
    assert!(records.contains("\texit_code=42\t"));
    assert!(records.contains("\tproc_state=Z\t"));
    assert!(records.contains("\twaitid_wait4_consistent=true\t"));
    assert!(records.contains("\ttracked_children_before_reap="));
    assert!(records.contains("\tsession_root_state="));
    assert!(records.contains("\tsession_root_observation_unix_ns="));
    assert!(records.contains("\tsession_root_observation_method="));
    assert!(records.contains("\tsession_root_terminal_at_child_observation="));
    let terminal = find_record(&records, "session-supervisor-terminated", |_| true);
    assert_eq!(
        journal_field(terminal, "journal_failures_after_spawn"),
        Some("0")
    );
    assert_eq!(
        journal_field(terminal, "journal_failure_recoveries"),
        Some("0")
    );
    assert_eq!(
        journal_field(terminal, "journal_failure_active"),
        Some("false")
    );
    assert_eq!(
        records
            .matches("\tevent=session-child-terminated\t")
            .count(),
        2
    );
}

#[test]
fn session_failure_report_retains_and_forwards_exact_process_output() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let report = directory.0.join("linuxcnc.report");
    fs::write(&report, b"STALE REPORT MUST NOT SURVIVE\n").expect("seed stale report");

    let output = Command::new(env!("CARGO_BIN_EXE_dmc2-session-supervisor"))
        .arg("--role")
        .arg("session-lifecycle-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--failure-report")
        .arg(&report)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg("printf 'HAL FILE ERROR: broken.hal:27 duplicate pin\\n' >&2; printf 'startup phase: HAL\\n'; exit 37")
        .output()
        .expect("run failure-report session");

    assert_eq!(output.status.code(), Some(37));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "startup phase: HAL\n"
    );
    let forwarded_stderr = String::from_utf8_lossy(&output.stderr);
    assert!(forwarded_stderr.contains("HAL FILE ERROR: broken.hal:27 duplicate pin"));
    assert!(forwarded_stderr.contains("full report:"));

    let report_text = fs::read_to_string(&report).expect("read automatic failure report");
    assert!(report_text.contains("identity: LINUXCNC_SESSION_EXITED_NONZERO"));
    assert!(report_text.contains("exit-code: 37"));
    assert!(report_text.contains("HAL FILE ERROR: broken.hal:27 duplicate pin"));
    assert!(report_text.contains("startup phase: HAL"));
    assert!(!report_text.contains("STALE REPORT MUST NOT SURVIVE"));

    let records = fs::read_to_string(&journal).expect("read report lifecycle journal");
    let terminal = find_record(&records, "session-supervisor-terminated", |_| true);
    assert_eq!(
        journal_field(terminal, "failure_report_written"),
        Some("true")
    );
    assert_ne!(
        journal_field(terminal, "failure_report_path_hex"),
        Some("NONE")
    );
}

#[test]
fn session_subreaper_retains_a_rolling_snapshot_for_an_adopted_descendant() {
    use std::os::unix::ffi::OsStrExt;

    let directory = TestDirectory::new();
    let journal = directory.journal();
    let tracked_file = directory.0.join("adopted-descendant-open-file");
    let release = directory.0.join("adopted-open-descriptor-now");
    fs::write(&tracked_file, b"snapshot target").expect("create adopted descriptor target");
    let script = format!(
        "(attempt=0; while [ ! -e {} ] && [ \"$attempt\" -lt 400 ]; do attempt=$((attempt + 1)); sleep 0.005; done; [ -e {} ] || exit 99; exec 9<{}; sleep 0.05; exit 42) & exit 7",
        release.display(),
        release.display(),
        tracked_file.display()
    );
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_dmc2-session-supervisor"))
        .arg("--role")
        .arg("session-lifecycle-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script)
        .spawn()
        .expect("spawn adopted rolling-snapshot test");
    assert!(
        wait_for_journal_record(&journal, |line| {
            line.contains("\tevent=session-child-observed\t")
                && line.contains("\trelation=adopted-session-descendant\t")
        }),
        "session owner never committed its initial adopted-child observation"
    );
    fs::write(&release, b"release").expect("release adopted snapshot child");
    let status = supervisor
        .wait()
        .expect("wait for adopted rolling-snapshot owner");

    assert_eq!(status.code(), Some(7));
    let records = fs::read_to_string(&journal).expect("read adopted rolling-snapshot journal");
    assert_complete_records(&records);
    let terminal = records
        .lines()
        .find(|line| {
            line.contains("\tevent=session-child-terminal-observed\t")
                && line.contains("\twaitid_status=42\t")
        })
        .expect("adopted descendant terminal-before-reap record");
    let expected_target = encode_hex(tracked_file.as_os_str().as_bytes());
    assert!(
        terminal.contains("\tlast_live_snapshot_state=captured\t"),
        "{terminal}"
    );
    assert!(terminal.contains(&expected_target), "{terminal}");
}

#[test]
fn session_subreaper_marks_a_descendant_lost_while_linuxcnc_is_still_running() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let status = Command::new(env!("CARGO_BIN_EXE_dmc2-session-supervisor"))
        .arg("--role")
        .arg("session-lifecycle-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg("( (sleep 0.05; exit 42) & exit 0 ) & sleep 0.20; exit 7")
        .status()
        .expect("run live-session descendant-loss test");

    assert_eq!(status.code(), Some(7));
    let records = fs::read_to_string(&journal).expect("read session lifecycle journal");
    assert_complete_records(&records);
    let departed = records
        .lines()
        .find(|line| {
            line.contains("\tevent=session-child-terminated\t") && line.contains("\texit_code=42\t")
        })
        .expect("adopted descendant terminal record");
    assert!(departed.contains("\tsession_root_state=nonterminal-at-probe\t"));
    assert!(departed.contains("\tsession_root_terminal_at_child_observation=false\t"));
}

#[test]
fn session_subreaper_identifies_an_adopted_direct_process_owner() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let owner = env!("CARGO_BIN_EXE_dmc2-process-supervisor");
    let script = format!(
        "{owner} --role lifecycle-test --journal {} -- /bin/sh -c 'sleep 0.25; exit 42' & exit 7",
        journal.display()
    );
    let status = Command::new(env!("CARGO_BIN_EXE_dmc2-session-supervisor"))
        .arg("--role")
        .arg("session-lifecycle-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script)
        .status()
        .expect("run layered lifecycle ownership test");

    assert_eq!(status.code(), Some(7));
    let records = fs::read_to_string(&journal).expect("read layered lifecycle journal");
    assert_complete_records(&records);
    assert!(records.contains("\trole=lifecycle-test\t"));
    assert!(
        records.contains("\tlayer=direct-process-owner\t"),
        "{records}"
    );
    assert!(
        records.contains("\tterminal_identity_source=supervisor-process-name\t"),
        "{records}"
    );
    assert!(records.contains("\tterminal_identity_matches_initial=true\t"));
    assert!(records.contains("\tevent=process-terminated\t"));
    assert!(records.contains("\texit_code=42\t"));
}

#[test]
fn session_subreaper_recovers_a_crashing_workload_after_its_direct_owner_dies() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let ready = directory.0.join("orphaned-workload-ready");
    let owner = env!("CARGO_BIN_EXE_dmc2-process-supervisor");
    let script = format!(
        "{owner} --role lifecycle-test --journal {} -- /bin/sh -c ': > {}; sleep 0.10; kill -SEGV $$' & \
         owner_pid=$!; \
         (attempt=0; \
          while [ ! -e {} ] && [ \"$attempt\" -lt 200 ]; do \
              attempt=$((attempt + 1)); sleep 0.005; \
          done; \
          [ -e {} ] || exit 99; \
          kill -KILL \"$owner_pid\") & \
         exit 7",
        journal.display(),
        ready.display(),
        ready.display(),
        ready.display(),
    );
    let status = Command::new(env!("CARGO_BIN_EXE_dmc2-session-supervisor"))
        .current_dir(&directory.0)
        .arg("--role")
        .arg("session-lifecycle-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script)
        .status()
        .expect("run direct-owner-loss lifecycle test");

    assert_eq!(status.code(), Some(7));
    let records = fs::read_to_string(&journal).expect("read owner-loss lifecycle journal");
    assert_complete_records(&records);
    let owner_terminal = records
        .lines()
        .find(|line| {
            line.contains("\tevent=session-child-terminated\t")
                && line.contains("\trole=lifecycle-test\t")
                && line.contains("\tlayer=direct-process-owner\t")
        })
        .unwrap_or_else(|| {
            let summaries = records
                .lines()
                .filter(|line| journal_field(line, "event") == Some("session-child-terminated"))
                .map(|line| {
                    [
                        "child_pid",
                        "role",
                        "layer",
                        "identity_source",
                        "terminal_role",
                        "terminal_layer",
                        "terminal_identity_source",
                        "signal",
                        "exit_code",
                    ]
                    .map(|name| (name, journal_field(line, name).unwrap_or("MISSING")))
                })
                .collect::<Vec<_>>();
            panic!("direct owner terminal record missing: {summaries:?}")
        });
    assert!(owner_terminal.contains("\tsignal=9\t"), "{owner_terminal}");
    let workload_terminal = records
        .lines()
        .find(|line| {
            line.contains("\tevent=session-child-terminated\t") && line.contains("\tsignal=11\t")
        })
        .expect("orphaned workload terminal record");
    assert!(
        workload_terminal.contains("\trelation=adopted-session-descendant\t"),
        "{workload_terminal}"
    );
    assert!(
        workload_terminal.contains("\tcore_dumped=true\t"),
        "{workload_terminal}"
    );
    assert!(
        workload_terminal.contains("\tcore_artifact_state=captured\t"),
        "{workload_terminal}"
    );
    let copy = workload_terminal
        .split('\t')
        .find_map(|field| field.strip_prefix("core_artifact_copy_hex="))
        .map(decode_hex_path)
        .expect("orphaned workload durable core path");
    let metadata = fs::metadata(copy).expect("orphaned workload durable core copy");
    assert!(metadata.is_file());
    assert!(metadata.len() > 0);
}

#[test]
fn session_subreaper_preserves_a_real_adopted_descendant_core() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let status = Command::new(env!("CARGO_BIN_EXE_dmc2-session-supervisor"))
        .current_dir(&directory.0)
        .arg("--role")
        .arg("session-lifecycle-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg("/bin/sh -c 'sleep 0.05; kill -SEGV $$' & exit 7")
        .status()
        .expect("run adopted descendant kernel-core test");

    assert_eq!(status.code(), Some(7));
    let records = fs::read_to_string(&journal).expect("read adopted-core lifecycle journal");
    assert_complete_records(&records);
    let terminal = records
        .lines()
        .find(|line| {
            line.contains("\tevent=session-child-terminated\t") && line.contains("\tsignal=11\t")
        })
        .expect("adopted crashing descendant terminal record");
    assert!(terminal.contains("\tcore_dumped=true\t"), "{terminal}");
    assert!(
        terminal.contains("\tcore_artifact_state=captured\t"),
        "{terminal}"
    );
    assert!(terminal.contains("\trelation=adopted-session-descendant\t"));
    assert!(
        terminal.contains("\tcore_cwd_selected_source=proc-cwd-after-process-observation\t"),
        "{terminal}"
    );
}

#[test]
fn live_task_line_preserves_the_supervised_milltask_invocation() {
    let ini_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../live/dmc2.ini");
    let ini = fs::read_to_string(ini_path).expect("read live DMC2 INI");
    let task = ini
        .lines()
        .find_map(|line| line.strip_prefix("TASK = "))
        .expect("live TASK entry");
    let mut supervisor_arguments = task
        .split_ascii_whitespace()
        .skip(1)
        .map(OsString::from)
        .collect::<Vec<_>>();
    supervisor_arguments.extend([OsString::from("-ini"), OsString::from("dmc2.ini")]);
    let invocation = Invocation::parse(supervisor_arguments).expect("parse configured task");

    assert_eq!(invocation.role.name(), "milltask");
    assert_eq!(invocation.program, OsString::from("/usr/bin/milltask"));
    assert_eq!(
        invocation.arguments,
        [OsString::from("-ini"), OsString::from("dmc2.ini")]
    );
}

#[test]
fn live_display_line_preserves_linuxcncs_prepended_ini_arguments() {
    let ini_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../live/dmc2.ini");
    let ini = fs::read_to_string(ini_path).expect("read live DMC2 INI");
    let display = ini
        .lines()
        .find_map(|line| line.strip_prefix("DISPLAY = "))
        .expect("live DISPLAY entry");
    let mut supervisor_arguments = vec![OsString::from("-ini"), OsString::from("dmc2.ini")];
    supervisor_arguments.extend(display.split_ascii_whitespace().skip(1).map(OsString::from));
    let invocation = Invocation::parse(supervisor_arguments).expect("parse configured display");

    assert_eq!(invocation.role.name(), "axis");
    assert_eq!(invocation.program, OsString::from("/usr/bin/axis"));
    assert_eq!(
        invocation.arguments,
        [OsString::from("-ini"), OsString::from("dmc2.ini")]
    );
}

fn assert_real_caught_signal(kill_argument: &str, signal: i32, signal_name: &str) {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let ready = directory.0.join("signal-handler-ready");
    let library = directory.0.join("libdmc2_signal_evidence.so");
    let fixture = directory.0.join("dmc2-signal-fixture");
    compile_signal_evidence_fixture(&library, &fixture);

    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_dmc2-process-supervisor"))
        .current_dir(&directory.0)
        .env("DMC2_SIGNAL_EVIDENCE_TEST_LIBRARY", &library)
        .arg("--role")
        .arg("signal-evidence-test")
        .arg("--journal")
        .arg(&journal)
        .arg("--")
        .arg("./dmc2-signal-fixture")
        .arg(&ready)
        .spawn()
        .expect("spawn caught-signal supervisor");
    assert!(
        wait_for_path(&ready),
        "fixture never confirmed both handlers were installed"
    );
    let child_pid = wait_for_journal_field(&journal, "process-started", "child_pid")
        .and_then(|value| value.parse::<u32>().ok())
        .expect("supervisor journal contains the real fixture PID");

    let mut sender = Command::new("/bin/kill")
        .arg(kill_argument)
        .arg(child_pid.to_string())
        .spawn()
        .expect("spawn real signal sender");
    let sender_pid = sender.id();
    assert!(sender
        .wait()
        .expect("wait for real signal sender")
        .success());
    let status = supervisor
        .wait()
        .expect("wait for caught-signal supervisor");

    assert_eq!(status.code(), Some(0));
    let records = fs::read_to_string(&journal).expect("read caught-signal journal");
    assert_complete_records(&records);
    let plan = find_record(&records, "supervisor-started", |_| true);
    let target = fs::canonicalize(&fixture).expect("canonical signal fixture path");
    let expected_target = encode_hex(target.as_os_str().as_bytes());
    assert_eq!(
        journal_field(plan, "caught_signal_target_path_hex"),
        Some(expected_target.as_str())
    );
    assert_eq!(
        journal_field(plan, "caught_signal_delivery_record_semantics"),
        Some("best-effort-nonblocking")
    );
    assert_eq!(
        journal_field(plan, "caught_signal_delivery_drop_counter_available"),
        Some("false")
    );
    let initialization = find_record(&records, "caught-signal-evidence-record", |line| {
        journal_field(line, "caught_signal_record_kind") == Some("initialized")
    });
    assert_eq!(
        journal_field(initialization, "caught_signal_target_pid"),
        Some(child_pid.to_string().as_str())
    );
    for expected_signal in [2, 15] {
        let expected = expected_signal.to_string();
        find_record(&records, "caught-signal-evidence-record", |line| {
            journal_field(line, "caught_signal_record_kind") == Some("handler-armed")
                && journal_field(line, "caught_signal") == Some(expected.as_str())
                && journal_field(line, "caught_signal_target_matches") == Some("true")
        });
    }
    let delivered = find_record(&records, "caught-signal-evidence-record", |line| {
        journal_field(line, "caught_signal_record_kind") == Some("signal-delivered")
    });
    assert_eq!(
        journal_field(delivered, "caught_signal"),
        Some(signal.to_string().as_str())
    );
    assert_eq!(
        journal_field(delivered, "caught_signal_name"),
        Some(signal_name)
    );
    assert_eq!(journal_field(delivered, "caught_signal_code"), Some("0"));
    assert_eq!(
        journal_field(delivered, "caught_signal_code_name"),
        Some("SI_USER")
    );
    assert_eq!(
        journal_field(delivered, "caught_signal_sender_pid"),
        Some(sender_pid.to_string().as_str())
    );
    assert_eq!(
        journal_field(delivered, "caught_signal_sender_uid"),
        Some(
            fs::metadata("/proc/self")
                .expect("self metadata")
                .uid()
                .to_string()
                .as_str()
        )
    );
    assert_eq!(
        journal_field(delivered, "caught_signal_sender_identity_valid"),
        Some("true")
    );
    assert_eq!(
        journal_field(delivered, "caught_signal_target_pid"),
        Some(child_pid.to_string().as_str())
    );
    assert_eq!(
        journal_field(delivered, "caught_signal_target_matches"),
        Some("true")
    );
    let terminal = find_record(&records, "process-terminated", |_| true);
    assert_eq!(journal_field(terminal, "exit_code"), Some("0"));
    assert_eq!(
        journal_field(terminal, "caught_signal_evidence_state"),
        Some("ready")
    );
    assert_eq!(
        journal_field(terminal, "caught_signal_matching_delivered_records"),
        Some("1")
    );
    assert_eq!(
        journal_field(terminal, "caught_signal_channel_eof"),
        Some("true")
    );
    assert_eq!(
        journal_field(terminal, "caught_signal_invalid_records"),
        Some("0")
    );
    assert_eq!(
        journal_field(terminal, "caught_signal_read_failures"),
        Some("0")
    );
    assert_eq!(
        journal_field(terminal, "caught_signal_malformed_packets"),
        Some("0")
    );
    assert_eq!(
        journal_field(terminal, "caught_signal_current_armed_signals"),
        Some("2,15")
    );
    assert_eq!(
        journal_field(terminal, "last_caught_signal_sender_pid"),
        Some(sender_pid.to_string().as_str())
    );
    assert_eq!(
        journal_field(terminal, "outcome"),
        Some("caught-signal-clean-exit")
    );
}

fn compile_signal_evidence_fixture(library: &Path, fixture: &Path) {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    compile_shared_c_fixture(
        &project.join("native_src/dmc2_signal_evidence.c"),
        library,
        "signal interposer",
    );
    let fixture_status = Command::new("cc")
        .args(C_FIXTURE_COMMON_FLAGS)
        .arg("-o")
        .arg(fixture)
        .arg(project.join("tests/fixtures/caught_signal.c"))
        .status()
        .expect("compile exact caught-signal fixture source");
    assert!(
        fixture_status.success(),
        "signal fixture compilation failed"
    );
    fs::set_permissions(fixture, fs::Permissions::from_mode(0o755))
        .expect("make signal target non-writable by group/world");
}

fn compile_wait_fault_fixture(library: &Path) {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    compile_shared_c_fixture(
        &project.join("tests/fixtures/wait_fault_injection.c"),
        library,
        "wait fault-injection interposer",
    );
}

const C_FIXTURE_COMMON_FLAGS: &[&str] = &[
    "-std=c11",
    "-O2",
    "-fstack-protector-strong",
    "-D_FORTIFY_SOURCE=3",
    "-Wall",
    "-Wextra",
    "-Wpedantic",
    "-Werror",
];

fn compile_shared_c_fixture(source: &Path, library: &Path, description: &str) {
    let status = Command::new("cc")
        .args(C_FIXTURE_COMMON_FLAGS)
        .args([
            "-fPIC",
            "-fvisibility=hidden",
            "-shared",
            "-Wl,-z,defs",
            "-Wl,-z,relro,-z,now,-z,noexecstack",
            "-o",
        ])
        .arg(library)
        .arg(source)
        .arg("-ldl")
        .status()
        .unwrap_or_else(|error| panic!("execute {description} compiler: {error}"));
    assert!(status.success(), "{description} compilation failed");
    fs::set_permissions(library, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|error| panic!("set {description} permissions: {error}"));
}

fn wait_for_path(path: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if path.is_file() {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    false
}

fn wait_for_journal_field(journal: &Path, event: &str, field: &str) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Ok(records) = fs::read_to_string(journal) {
            if let Some(value) = records.lines().find_map(|line| {
                (journal_field(line, "event") == Some(event))
                    .then(|| journal_field(line, field))
                    .flatten()
            }) {
                return Some(value.to_owned());
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    None
}

fn wait_for_journal_field_for_role(
    journal: &Path,
    event: &str,
    role: &str,
    field: &str,
) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Ok(records) = fs::read_to_string(journal) {
            if let Some(value) = records.lines().find_map(|line| {
                (journal_field(line, "event") == Some(event)
                    && journal_field(line, "role") == Some(role))
                .then(|| journal_field(line, field))
                .flatten()
            }) {
                return Some(value.to_owned());
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    None
}

fn send_signal_and_wait(signal: &str, pid: u32) {
    let status = Command::new("/bin/kill")
        .arg(signal)
        .arg(pid.to_string())
        .status()
        .expect("execute test signal sender");
    assert!(status.success(), "test signal sender failed for PID {pid}");
}

fn find_record<'a>(records: &'a str, event: &str, predicate: impl Fn(&str) -> bool) -> &'a str {
    records
        .lines()
        .find(|line| journal_field(line, "event") == Some(event) && predicate(line))
        .unwrap_or_else(|| panic!("missing {event} record matching predicate:\n{records}"))
}

fn run_supervisor(journal: &Path, script: &str) -> ExitStatus {
    spawn_supervisor(journal, script)
        .wait()
        .expect("run lifecycle supervisor")
}

fn spawn_supervisor(journal: &Path, script: &str) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dmc2-process-supervisor"));
    command
        .arg("--role")
        .arg("lifecycle-test")
        .arg("--journal")
        .arg(journal)
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script);
    command.spawn().expect("spawn lifecycle supervisor")
}

fn wait_for_journal_record(journal: &Path, predicate: impl Fn(&str) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if fs::read_to_string(journal).is_ok_and(|records| records.lines().any(&predicate)) {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    false
}

fn journal_field<'a>(record: &'a str, name: &str) -> Option<&'a str> {
    record.split('\t').find_map(|field| {
        field
            .strip_prefix(name)
            .and_then(|value| value.strip_prefix('='))
    })
}

fn assert_complete_records(records: &str) {
    use std::collections::BTreeSet;

    assert!(records.ends_with('\n'));
    let lines = records.lines().collect::<Vec<_>>();
    let mut offset = 0_u64;
    for (index, record) in lines.iter().copied().enumerate() {
        if !has_valid_record_checksum(record) {
            let recovery = lines
                .get(index + 1)
                .copied()
                .expect("interrupted record is followed by its recovery record");
            assert_eq!(
                journal_field(recovery, "event"),
                Some("journal-partial-record-separated"),
                "unaccounted interrupted lifecycle record: {record}"
            );
            assert_eq!(
                journal_field(recovery, "interrupted_record_offset")
                    .and_then(|value| value.parse::<u64>().ok()),
                Some(offset),
                "recovery offset does not identify interrupted record"
            );
            assert_eq!(
                journal_field(recovery, "interrupted_record_bytes")
                    .and_then(|value| value.parse::<usize>().ok()),
                Some(record.len()),
                "recovery length does not identify interrupted record"
            );
            assert_eq!(
                journal_field(recovery, "interrupted_record_crc32")
                    .and_then(|value| u32::from_str_radix(value, 16).ok()),
                Some(crc32(record.as_bytes())),
                "recovery checksum does not identify interrupted record"
            );
            offset = offset.saturating_add(record.len() as u64 + 1);
            continue;
        }
        assert!(record.starts_with("schema=dmc2-process-lifecycle-v1\t"));
        let mut field_names = BTreeSet::new();
        for field in record.split('\t') {
            let name = field
                .split_once('=')
                .map(|(name, _)| name)
                .expect("journal field contains an equals sign");
            assert!(
                field_names.insert(name),
                "duplicate lifecycle-journal field {name:?}: {record}"
            );
        }
        offset = offset.saturating_add(record.len() as u64 + 1);
    }
}

fn has_valid_record_checksum(record: &str) -> bool {
    let Some((payload, checksum)) = record.rsplit_once("\tcrc32=") else {
        return false;
    };
    checksum.len() == 8
        && u32::from_str_radix(checksum, 16)
            .is_ok_and(|expected| crc32(payload.as_bytes()) == expected)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn decode_hex_path(value: &str) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;

    assert_eq!(value.len() % 2, 0, "hex path has complete bytes");
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).expect("ASCII hex path");
            u8::from_str_radix(pair, 16).expect("valid hex path")
        })
        .collect();
    PathBuf::from(OsString::from_vec(bytes))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}
