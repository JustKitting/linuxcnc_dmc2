use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FailureCode {
    Argument,
    Artifact,
    DeploymentIdentity,
    EstopContract,
    FileSystem,
    HalCommand,
    JournalPresentation,
    LinuxCncLaunch,
    LinuxCncProtocol,
    LinuxCncVersion,
    MotionNotObserved,
    PendantTransport,
    RealtimeBusy,
    Timeout,
}

impl FailureCode {
    const fn name(self) -> &'static str {
        match self {
            Self::Argument => "ACCEPTANCE_ARGUMENT_INVALID",
            Self::Artifact => "ACCEPTANCE_ARTIFACT_INVALID",
            Self::DeploymentIdentity => "DEPLOYMENT_IDENTITY_MISMATCH",
            Self::EstopContract => "CANONICAL_ESTOP_CONTRACT_FAILURE",
            Self::FileSystem => "ACCEPTANCE_FILESYSTEM_FAILURE",
            Self::HalCommand => "ACCEPTANCE_HAL_COMMAND_FAILURE",
            Self::JournalPresentation => "AXIS_ERROR_JOURNAL_ACCEPTANCE_FAILURE",
            Self::LinuxCncLaunch => "ACCEPTANCE_LINUXCNC_LAUNCH_FAILURE",
            Self::LinuxCncProtocol => "ACCEPTANCE_LINUXCNCRSH_PROTOCOL_FAILURE",
            Self::LinuxCncVersion => "ACCEPTANCE_LINUXCNC_VERSION_MISMATCH",
            Self::MotionNotObserved => "REAL_LINUXCNC_MOTION_NOT_OBSERVED",
            Self::PendantTransport => "ACCEPTANCE_PENDANT_TRANSPORT_FAILURE",
            Self::RealtimeBusy => "REAL_LINUXCNC_MOTION_TEST_BLOCKED",
            Self::Timeout => "ACCEPTANCE_TIMEOUT",
        }
    }
}

#[derive(Debug)]
pub(crate) struct Failure {
    code: FailureCode,
    detail: String,
}

impl Failure {
    pub(crate) fn new(code: FailureCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }

    pub(crate) fn io(code: FailureCode, operation: &'static str, error: std::io::Error) -> Self {
        Self::new(code, format!("operation={operation}; error={error}"))
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code.name(), self.detail)
    }
}

impl std::error::Error for Failure {}

pub(crate) type Result<T> = std::result::Result<T, Failure>;
