use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::atomic::{AtomicU64, Ordering};

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
    assert!(records.contains("\tevent=process-terminated\t"));
    assert!(records.contains("\trole=lifecycle-test\t"));
    assert!(records.contains("\texit_code=23\t"));
    assert!(records.contains("\tsignal=NONE\t"));
    assert!(records.contains("\tproc_parent_pid="));
    assert!(records.contains("\tproc_start_time_ticks="));
    assert!(records.contains("\trusage_max_resident_kib="));
    assert!(records.contains("\toutcome=nonzero-exit\tcrc32="));
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
    assert!(records.contains("\tcore_dumped="));
    assert!(records.contains("\toutcome=kernel-signal-termination\tcrc32="));
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
    assert_eq!(records.lines().count(), CHILDREN * 3);
    assert_eq!(
        records.matches("\tevent=process-terminated\t").count(),
        CHILDREN
    );
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
    assert!(records.contains("\trole=session-lifecycle-test\t"));
    assert!(records.contains("\trelation=direct-session-child\t"));
    assert!(records.contains("\trelation=adopted-session-descendant\t"));
    assert!(records.contains("\texit_code=7\t"));
    assert!(records.contains("\texit_code=42\t"));
    assert_eq!(
        records
            .matches("\tevent=session-child-terminated\t")
            .count(),
        2
    );
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
    assert!(records.contains("\tlayer=direct-process-owner\t"));
    assert!(
        records.contains("\tidentity_source=supervisor-command-line-role\t"),
        "{records}"
    );
    assert!(records.contains("\tevent=process-terminated\t"));
    assert!(records.contains("\texit_code=42\t"));
}

#[test]
fn every_configurable_long_lived_process_uses_the_generic_owner() {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let ini = fs::read_to_string(project.join("live/dmc2.ini")).expect("read live DMC2 INI");
    let hal = fs::read_to_string(project.join("live/pendant.hal")).expect("read pendant HAL");
    let configuration = format!("{ini}\n{hal}");
    let catalog =
        fs::read_to_string(project.join("config/processes.tsv")).expect("read process catalog");
    let mut checked = 0_usize;
    for line in catalog.lines().skip(2).filter(|line| !line.is_empty()) {
        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 7, "invalid process-catalog row: {line}");
        let [role, program, _, ownership, criticality, _, _] = fields.as_slice() else {
            unreachable!("field count checked above")
        };
        if *ownership != "direct-child" || *criticality == "verification-only" {
            continue;
        }
        let marker = format!(
            "../native/bin/dmc2-process-supervisor --role {role} --journal ../var/log/linuxcnc/process-lifecycle.tsv -- {program}"
        );
        assert!(
            configuration.contains(&marker),
            "missing configured owner for catalogued direct-child role {role}"
        );
        checked += 1;
    }
    assert!(
        checked > 0,
        "process catalog has no production direct children"
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

fn assert_complete_records(records: &str) {
    assert!(records.ends_with('\n'));
    for record in records.lines() {
        assert!(record.starts_with("schema=dmc2-process-lifecycle-v1\t"));
        let (payload, checksum) = record
            .rsplit_once("\tcrc32=")
            .expect("record checksum field");
        assert_eq!(checksum.len(), 8);
        assert_eq!(
            u32::from_str_radix(checksum, 16).expect("hex checksum"),
            crc32(payload.as_bytes())
        );
    }
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
