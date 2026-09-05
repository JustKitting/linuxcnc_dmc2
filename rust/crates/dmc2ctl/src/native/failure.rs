use super::ffi;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandFailure {
    InvalidArgument,
    OpenFailed,
    StatusFailed,
    WriteFailed,
    TimedOut,
    Rejected,
    Unknown(u32),
}

impl CommandFailure {
    pub const fn from_raw(value: u32) -> Self {
        match value {
            ffi::DMC2_CONTROL_INVALID_ARGUMENT => Self::InvalidArgument,
            ffi::DMC2_CONTROL_OPEN_FAILED => Self::OpenFailed,
            ffi::DMC2_CONTROL_STATUS_FAILED => Self::StatusFailed,
            ffi::DMC2_CONTROL_WRITE_FAILED => Self::WriteFailed,
            ffi::DMC2_CONTROL_TIMEOUT => Self::TimedOut,
            ffi::DMC2_CONTROL_REJECTED => Self::Rejected,
            other => Self::Unknown(other),
        }
    }

    pub const fn wire_code(self) -> u32 {
        match self {
            Self::InvalidArgument => ffi::DMC2_CONTROL_INVALID_ARGUMENT,
            Self::OpenFailed => ffi::DMC2_CONTROL_OPEN_FAILED,
            Self::StatusFailed => ffi::DMC2_CONTROL_STATUS_FAILED,
            Self::WriteFailed => ffi::DMC2_CONTROL_WRITE_FAILED,
            Self::TimedOut => ffi::DMC2_CONTROL_TIMEOUT,
            Self::Rejected => ffi::DMC2_CONTROL_REJECTED,
            Self::Unknown(value) => value,
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::InvalidArgument => "the control adapter rejected invalid command arguments or an unavailable session; restore the matching application before retrying",
            Self::OpenFailed => "the LinuxCNC command session could not be opened; restore the session through Applications",
            Self::StatusFailed => "LinuxCNC command status could not be read; acceptance is unknown; inspect the UI diagnostic and use Abort before retrying",
            Self::WriteFailed => "the command could not be written to LinuxCNC; inspect the UI transport diagnostic before retrying",
            Self::TimedOut => "LinuxCNC did not acknowledge the requested command outcome before the deadline; acceptance is unknown; use Abort and inspect the UI diagnostic before retrying",
            Self::Rejected => "LinuxCNC explicitly rejected this command; read the operator error in AXIS, correct its cause, then use the listed recovery controls before an explicit retry",
            Self::Unknown(_) => "the adapter returned an unknown failure value; command acceptance is not established; use Abort and restore the matching application",
        }
    }
}
