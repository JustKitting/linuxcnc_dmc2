use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use dmc2_milltask_supervisor::Invocation;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "dmc2-milltask-supervisor-test-{}-{sequence}",
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
fn records_the_kernel_exit_code_from_a_real_child() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let status = run_supervisor(&journal, &["/bin/sh", "-c", "exit 23"]);

    assert_eq!(status.code(), Some(23));
    let records = fs::read_to_string(&journal).expect("read lifecycle journal");
    assert_complete_records(&records);
    assert!(records.contains("\tevent=milltask-started\t"));
    assert!(records.contains("\tevent=milltask-terminated\t"));
    assert!(records.contains("\texit_code=23\t"));
    assert!(records.contains("\tsignal=NONE\t"));
    assert!(records.contains("\toutcome=nonzero-exit\tcrc32="));
}

#[test]
fn records_the_kernel_signal_from_a_real_child() {
    let directory = TestDirectory::new();
    let journal = directory.journal();
    let status = run_supervisor(&journal, &["/bin/sh", "-c", "ulimit -c 0; kill -ABRT $$"]);

    assert_eq!(status.code(), Some(134));
    let records = fs::read_to_string(&journal).expect("read lifecycle journal");
    assert_complete_records(&records);
    assert!(records.contains("\texit_code=NONE\t"));
    assert!(records.contains("\tsignal=6\t"));
    assert!(records.contains("\tsignal_name=SIGABRT\t"));
    assert!(records.contains("\toutcome=kernel-signal-termination\tcrc32="));
}

#[test]
fn live_task_line_preserves_the_supervised_milltask_invocation() {
    let ini_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../live/dmc2.ini");
    let ini = fs::read_to_string(ini_path).expect("read live DMC2 INI");
    let task = ini
        .lines()
        .find_map(|line| line.strip_prefix("TASK = "))
        .expect("live TASK entry");
    let mut words = task.split_ascii_whitespace();

    assert_eq!(words.next(), Some("../native/bin/dmc2-milltask-supervisor"));
    let mut supervisor_arguments = words.map(OsString::from).collect::<Vec<_>>();
    supervisor_arguments.extend([OsString::from("-ini"), OsString::from("dmc2.ini")]);
    let invocation = Invocation::parse(supervisor_arguments).expect("parse configured task");

    assert_eq!(invocation.program, OsString::from("/usr/bin/milltask"));
    assert_eq!(
        invocation.arguments,
        [OsString::from("-ini"), OsString::from("dmc2.ini")]
    );
}

fn run_supervisor(journal: &Path, child: &[&str]) -> std::process::ExitStatus {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dmc2-milltask-supervisor"));
    command.arg("--journal").arg(journal).arg("--");
    command.args(child);
    command.status().expect("run lifecycle supervisor")
}

fn assert_complete_records(records: &str) {
    assert!(records.ends_with('\n'));
    for record in records.lines() {
        assert!(record.starts_with("schema=dmc2-milltask-lifecycle-v1\t"));
        assert!(record.contains("\tcrc32="));
    }
}
