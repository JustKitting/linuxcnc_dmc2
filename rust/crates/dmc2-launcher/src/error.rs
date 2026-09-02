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

    /// Stable identity for every launcher refusal or failure.
    pub const fn identity(&self) -> &'static str {
        match self {
            Self::Usage(_) => "LAUNCHER_USAGE_INVALID",
            Self::OperatingSystem { .. } => "OPERATING_SYSTEM_OPERATION_FAILED",
            Self::ProjectRootNotFound(_) => "DMC2_PROJECT_ROOT_NOT_FOUND",
            Self::NotRegularFile(_) => "REQUIRED_REGULAR_FILE_UNAVAILABLE",
            Self::EmbeddedFileChanged(_) => "COMPILED_LAUNCH_INPUT_CHANGED",
            Self::DeploymentMismatch { .. } => "DEPLOYED_BINARY_MISMATCH",
            Self::ExecutableUnavailable(_) => "REQUIRED_EXECUTABLE_UNAVAILABLE",
            Self::ProcessFailed { .. } => "EXTERNAL_PROCESS_FAILED",
            Self::LinuxCncVersion { .. } => "LINUXCNC_VERSION_MISMATCH",
            Self::OwnerConflict(_) => "LINUXCNC_OWNER_CONFLICT",
            Self::OwnerProbe { .. } => "LINUXCNC_OWNER_PROBE_FAILED",
            Self::ExecReturned => "PROCESS_REPLACEMENT_RETURNED",
        }
    }

    /// Concrete operator response paired with every launcher failure.
    pub const fn action(&self) -> &'static str {
        match self {
            Self::Usage(_) => "use the exact documented launcher command and arguments",
            Self::OperatingSystem { .. } => {
                "inspect the retained operation, target, OS code, and detail before retrying"
            }
            Self::ProjectRootNotFound(_) => {
                "run the deployed launcher from its intact DMC2 project installation"
            }
            Self::NotRegularFile(_) => {
                "restore the named required regular file from the verified project build"
            }
            Self::EmbeddedFileChanged(_) => {
                "rebuild and test the launcher against the exact live input before launching"
            }
            Self::DeploymentMismatch { .. } => {
                "rebuild the verified artifact; live launch synchronizes verified realtime modules automatically"
            }
            Self::ExecutableUnavailable(_) => {
                "restore the named executable from the pinned LinuxCNC system installation"
            }
            Self::ProcessFailed { .. } => {
                "inspect the retained program, raw exit status, stdout, and stderr; do not infer a meaning from the number alone"
            }
            Self::LinuxCncVersion { .. } => {
                "install and select exact LinuxCNC 2.9.10 before launching this build"
            }
            Self::OwnerConflict(_) => {
                "stop the named competing LinuxCNC, HAL, or Mesa owner before launching"
            }
            Self::OwnerProbe { .. } => {
                "inspect the retained probe pattern, raw status, stdout, and stderr before launching"
            }
            Self::ExecReturned => {
                "inspect the process-replacement boundary; launch remains refused"
            }
        }
    }

    fn cause(&self) -> String {
        match self {
            Self::Usage(message) => message.clone(),
            Self::OperatingSystem {
                operation,
                target,
                code,
                detail,
            } => format!(
                "{operation} failed for {} (raw-os-code={code:?}): {detail}",
                render_path(target)
            ),
            Self::ProjectRootNotFound(start) => format!(
                "the DMC2 project root cannot be located from {}",
                render_path(start)
            ),
            Self::NotRegularFile(path) => format!(
                "the required regular file is unavailable: {}",
                render_path(path)
            ),
            Self::EmbeddedFileChanged(path) => format!(
                "the live launch input differs from the compiled launcher: {}",
                render_path(path)
            ),
            Self::DeploymentMismatch { deployed, staged } => format!(
                "deployed {} does not byte-match {}",
                render_path(deployed),
                render_path(staged)
            ),
            Self::ExecutableUnavailable(name) => {
                format!("the required executable is unavailable: {name}")
            }
            Self::ProcessFailed {
                program,
                status,
                stdout,
                stderr,
            } => format!(
                "{} failed (raw-exit-status={status:?}, stdout={}, stderr={})",
                render_path(program),
                render_bytes(stdout),
                render_bytes(stderr)
            ),
            Self::LinuxCncVersion { stdout, stderr } => format!(
                "LinuxCNC version output is not exact 2.9.10\\n (stdout={}, stderr={})",
                render_bytes(stdout),
                render_bytes(stderr)
            ),
            Self::OwnerConflict(matches) => {
                let mut cause = "another LinuxCNC/HAL/Mesa owner may be active".to_owned();
                for item in matches {
                    use fmt::Write;
                    write!(
                        &mut cause,
                        "; pattern={:?} stdout={} stderr={}",
                        item.pattern,
                        render_bytes(&item.stdout),
                        render_bytes(&item.stderr)
                    )
                    .expect("writing to String cannot fail");
                }
                cause
            }
            Self::OwnerProbe {
                pattern,
                status,
                stdout,
                stderr,
            } => format!(
                concat!(
                    "process-owner exclusivity cannot be proven for {:?} ",
                    "(raw-exit-status={:?}, stdout={}, stderr={})"
                ),
                pattern,
                status,
                render_bytes(stdout),
                render_bytes(stderr)
            ),
            Self::ExecReturned => "process replacement returned unexpectedly".to_owned(),
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
        write!(
            formatter,
            "{}: {}; action: {}",
            self.identity(),
            self.cause(),
            self.action()
        )
    }
}

impl std::error::Error for Error {}
