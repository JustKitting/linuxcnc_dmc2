use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::embedded;
use crate::integrity;
use crate::layout::Layout;
use crate::platform::{CommandSpec, Platform, ProcessOutput};
use crate::validation::{EXPECTED_INTERFACE_AUDIT, EXPECTED_LINUXCNC_VERSION};

#[derive(Debug, Clone)]
pub enum FileEntry {
    Regular(Vec<u8>),
    Other,
    Failure(io::ErrorKind),
    ReadFailure(io::ErrorKind),
}

#[derive(Debug, Clone)]
pub enum RunOutcome {
    Output(ProcessOutput),
    Failure(io::ErrorKind),
}

#[derive(Debug, Clone)]
pub struct ExpectedRun {
    pub program: PathBuf,
    pub arguments: Vec<OsString>,
    pub outcome: RunOutcome,
}

pub struct MockPlatform {
    pub executable: PathBuf,
    pub executable_failure: Option<io::ErrorKind>,
    pub files: RefCell<BTreeMap<PathBuf, FileEntry>>,
    pub executables: BTreeMap<String, PathBuf>,
    pub executable_failures: BTreeMap<String, io::ErrorKind>,
    pub runs: RefCell<VecDeque<ExpectedRun>>,
    pub replacements: RefCell<Vec<CommandSpec>>,
    pub replacement_failure: RefCell<Option<io::ErrorKind>>,
}

impl MockPlatform {
    pub fn nominal() -> (Self, Layout) {
        let layout = Layout {
            project: PathBuf::from("/project"),
            h100: PathBuf::from("/h100_modbus"),
        };
        let mut files = BTreeMap::new();
        for file in embedded::FILES {
            let root = match file.root {
                embedded::Root::Project => &layout.project,
                embedded::Root::H100 => &layout.h100,
            };
            files.insert(
                root.join(file.relative),
                FileEntry::Regular(file.bytes.to_vec()),
            );
        }
        files.insert(
            layout.project.join("rust/Cargo.toml"),
            FileEntry::Regular(b"workspace".to_vec()),
        );
        for (index, (deployed, staged)) in integrity::deployments(&layout).into_iter().enumerate() {
            let bytes = format!("release-{index}").into_bytes();
            files.insert(deployed, FileEntry::Regular(bytes.clone()));
            files.insert(staged, FileEntry::Regular(bytes));
        }
        let executables = BTreeMap::from([
            (
                "linuxcnc_var".to_owned(),
                PathBuf::from("/usr/bin/linuxcnc_var"),
            ),
            ("linuxcnc".to_owned(), PathBuf::from("/usr/bin/linuxcnc")),
            ("pgrep".to_owned(), PathBuf::from("/usr/bin/pgrep")),
            (
                "systemd-run".to_owned(),
                PathBuf::from("/usr/bin/systemd-run"),
            ),
        ]);
        let platform = Self {
            executable: layout.project.join("native/bin/dmc2-linuxcnc"),
            executable_failure: None,
            files: RefCell::new(files),
            executables,
            executable_failures: BTreeMap::new(),
            runs: RefCell::new(VecDeque::new()),
            replacements: RefCell::new(Vec::new()),
            replacement_failure: RefCell::new(Some(io::ErrorKind::PermissionDenied)),
        };
        platform.push_nominal_validation(&layout);
        (platform, layout)
    }

    pub fn output(status: Option<i32>, stdout: &[u8], stderr: &[u8]) -> RunOutcome {
        RunOutcome::Output(ProcessOutput {
            status,
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
        })
    }

    pub fn push_run(&self, program: impl Into<PathBuf>, arguments: &[&str], outcome: RunOutcome) {
        self.runs.borrow_mut().push_back(ExpectedRun {
            program: program.into(),
            arguments: arguments.iter().map(OsString::from).collect(),
            outcome,
        });
    }

    pub fn push_nominal_validation(&self, layout: &Layout) {
        self.push_run(
            "/usr/bin/linuxcnc_var",
            &["LINUXCNCVERSION"],
            Self::output(Some(0), EXPECTED_LINUXCNC_VERSION, b""),
        );
        self.push_run(
            layout.project.join("native/bin/dmc2-task-monitor"),
            &["--validate"],
            Self::output(Some(0), EXPECTED_INTERFACE_AUDIT, b""),
        );
    }

    pub fn push_clear_owners(&self) {
        for pattern in crate::owner::CONFLICT_PATTERNS {
            self.push_run(
                "/usr/bin/pgrep",
                &["-f", pattern],
                Self::output(Some(1), b"", b""),
            );
        }
    }

    pub fn assert_runs_consumed(&self) {
        assert!(
            self.runs.borrow().is_empty(),
            "unconsumed process expectations"
        );
    }
}

impl Platform for MockPlatform {
    fn current_executable(&self) -> io::Result<PathBuf> {
        match self.executable_failure {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(self.executable.clone()),
        }
    }

    fn is_regular_file(&self, path: &Path) -> io::Result<bool> {
        match self.files.borrow().get(path) {
            Some(FileEntry::Regular(_)) => Ok(true),
            Some(FileEntry::Other) => Ok(false),
            Some(FileEntry::Failure(kind)) => Err(io::Error::from(*kind)),
            Some(FileEntry::ReadFailure(_)) => Ok(true),
            None => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }

    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        match self.files.borrow().get(path) {
            Some(FileEntry::Regular(bytes)) => Ok(bytes.clone()),
            Some(FileEntry::Other) => Err(io::Error::from(io::ErrorKind::InvalidInput)),
            Some(FileEntry::Failure(kind)) => Err(io::Error::from(*kind)),
            Some(FileEntry::ReadFailure(kind)) => Err(io::Error::from(*kind)),
            None => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }

    fn find_executable(&self, name: &str) -> io::Result<Option<PathBuf>> {
        match self.executable_failures.get(name) {
            Some(kind) => Err(io::Error::from(*kind)),
            None => Ok(self.executables.get(name).cloned()),
        }
    }

    fn run(&self, command: &CommandSpec) -> io::Result<ProcessOutput> {
        let expected = self
            .runs
            .borrow_mut()
            .pop_front()
            .expect("unexpected process invocation");
        assert_eq!(command.program, expected.program);
        assert_eq!(command.arguments, expected.arguments);
        match expected.outcome {
            RunOutcome::Output(output) => Ok(output),
            RunOutcome::Failure(kind) => Err(io::Error::from(kind)),
        }
    }

    fn replace_process(&self, command: &CommandSpec) -> io::Result<()> {
        self.replacements.borrow_mut().push(command.clone());
        match self.replacement_failure.borrow_mut().take() {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(()),
        }
    }
}
