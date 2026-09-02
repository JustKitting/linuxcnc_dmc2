use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub arguments: Vec<OsString>,
    pub environment: BTreeMap<OsString, OsString>,
    pub working_directory: Option<PathBuf>,
}

impl CommandSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            arguments: Vec::new(),
            environment: BTreeMap::new(),
            working_directory: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub status: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub trait Platform {
    fn current_executable(&self) -> io::Result<PathBuf>;
    fn is_regular_file(&self, path: &Path) -> io::Result<bool>;
    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn remove_file_if_exists(&self, path: &Path) -> io::Result<bool>;
    fn find_executable(&self, name: &str) -> io::Result<Option<PathBuf>>;
    fn run(&self, command: &CommandSpec) -> io::Result<ProcessOutput>;
    fn replace_process(&self, command: &CommandSpec) -> io::Result<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RealPlatform;

impl RealPlatform {
    fn command(specification: &CommandSpec) -> Command {
        let mut command = Command::new(&specification.program);
        command.args(&specification.arguments);
        command.envs(&specification.environment);
        if let Some(directory) = &specification.working_directory {
            command.current_dir(directory);
        }
        command
    }
}

impl Platform for RealPlatform {
    fn current_executable(&self) -> io::Result<PathBuf> {
        std::env::current_exe()
    }

    fn is_regular_file(&self, path: &Path) -> io::Result<bool> {
        Ok(fs::symlink_metadata(path)?.file_type().is_file())
    }

    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn remove_file_if_exists(&self, path: &Path) -> io::Result<bool> {
        match fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn find_executable(&self, name: &str) -> io::Result<Option<PathBuf>> {
        let path = Path::new(name);
        if path.components().count() > 1 {
            return executable_file(path).map(|executable| executable.then(|| path.to_path_buf()));
        }
        let Some(paths) = std::env::var_os("PATH") else {
            return Ok(None);
        };
        for directory in std::env::split_paths(&paths) {
            let candidate = directory.join(name);
            if executable_file(&candidate)? {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    fn run(&self, specification: &CommandSpec) -> io::Result<ProcessOutput> {
        let output = Self::command(specification)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()?;
        Ok(ProcessOutput {
            status: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    fn replace_process(&self, specification: &CommandSpec) -> io::Result<()> {
        let error = Self::command(specification).exec();
        Err(error)
    }
}

fn executable_file(path: &Path) -> io::Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_file() && metadata.permissions().mode() & 0o111 != 0),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}
