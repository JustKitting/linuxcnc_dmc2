use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerMatch {
    pub pattern: &'static str,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Usage(String),
    OperatingSystem {
        operation: &'static str,
        target: PathBuf,
        code: Option<i32>,
        detail: String,
    },
    ProjectRootNotFound(PathBuf),
    NotRegularFile(PathBuf),
    EmbeddedFileChanged(PathBuf),
    DeploymentMismatch {
        deployed: PathBuf,
        staged: PathBuf,
    },
    ExecutableUnavailable(&'static str),
    ProcessFailed {
        program: PathBuf,
        status: Option<i32>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    LinuxCncVersion {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    ProgramValidation {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    OwnerConflict(Vec<OwnerMatch>),
    OwnerProbe {
        pattern: &'static str,
        status: Option<i32>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    ExecReturned,
}

impl Error {
    pub fn os(operation: &'static str, target: PathBuf, error: std::io::Error) -> Self {
        Self::OperatingSystem {
            operation,
            target,
            code: error.raw_os_error(),
            detail: error.to_string(),
        }
    }
}

pub fn render_bytes(bytes: &[u8]) -> String {
    let mut rendered = String::with_capacity(bytes.len());
    for byte in bytes {
        match *byte {
            b' '..=b'~' if *byte != b'\\' => rendered.push(char::from(*byte)),
            b'\\' => rendered.push_str("\\\\"),
            b'\n' => rendered.push_str("\\n"),
            b'\r' => rendered.push_str("\\r"),
            b'\t' => rendered.push_str("\\t"),
            other => {
                use fmt::Write;
                write!(&mut rendered, "\\x{other:02x}").expect("writing to String cannot fail");
            }
        }
    }
    rendered
}

fn render_path(path: &Path) -> String {
    render_bytes(path.as_os_str().as_bytes())
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(formatter, "{message}"),
            Self::OperatingSystem {
                operation,
                target,
                code,
                detail,
            } => write!(
                formatter,
                "{operation} failed for {} (os-code={code:?}): {detail}",
                render_path(target)
            ),
            Self::ProjectRootNotFound(start) => write!(
                formatter,
                "cannot locate the DMC2 project root from {}",
                render_path(start)
            ),
            Self::NotRegularFile(path) => {
                write!(
                    formatter,
                    "required regular file is unavailable: {}",
                    render_path(path)
                )
            }
            Self::EmbeddedFileChanged(path) => write!(
                formatter,
                "live launch input differs from the offline-tested build: {}",
                render_path(path)
            ),
            Self::DeploymentMismatch { deployed, staged } => write!(
                formatter,
                "deployed {} does not byte-match {}",
                render_path(deployed),
                render_path(staged)
            ),
            Self::ExecutableUnavailable(name) => {
                write!(formatter, "required executable is unavailable: {name}")
            }
            Self::ProcessFailed {
                program,
                status,
                stdout,
                stderr,
            } => write!(
                formatter,
                "{} failed (status={status:?}, stdout={}, stderr={})",
                render_path(program),
                render_bytes(stdout),
                render_bytes(stderr)
            ),
            Self::LinuxCncVersion { stdout, stderr } => write!(
                formatter,
                "refusing LinuxCNC version output (stdout={}, stderr={}); expected exact 2.9.10\\n",
                render_bytes(stdout),
                render_bytes(stderr)
            ),
            Self::ProgramValidation { stdout, stderr } => write!(
                formatter,
                "compiled task-monitor validation output changed (stdout={}, stderr={})",
                render_bytes(stdout),
                render_bytes(stderr)
            ),
            Self::OwnerConflict(matches) => {
                let evidence = matches
                    .iter()
                    .map(|item| {
                        format!(
                            "; pattern={:?} stdout={} stderr={}",
                            item.pattern,
                            render_bytes(&item.stdout),
                            render_bytes(&item.stderr)
                        )
                    })
                    .collect::<String>();
                write!(
                    formatter,
                    "another LinuxCNC/HAL/Mesa owner may be active{evidence}"
                )
            }
            Self::OwnerProbe {
                pattern,
                status,
                stdout,
                stderr,
            } => write!(
                formatter,
                concat!(
                    "cannot prove process-owner exclusivity for {:?} ",
                    "(status={:?}, stdout={}, stderr={})"
                ),
                pattern,
                status,
                render_bytes(stdout),
                render_bytes(stderr)
            ),
            Self::ExecReturned => write!(formatter, "process replacement returned unexpectedly"),
        }
    }
}

impl std::error::Error for Error {}
