//! Closed diagnostic categories and their default recovery state machines.

use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u64)]
pub enum DiagnosticCategory {
    Abi = 1 << 0,
    TopRcs = 1 << 1,
    TaskRcs = 1 << 2,
    MotionRcs = 1 << 3,
    TrajectoryRcs = 1 << 4,
    JointRcs = 1 << 5,
    AxisRcs = 1 << 6,
    SpindleRcs = 1 << 7,
    IoRcs = 1 << 8,
    TaskExec = 1 << 9,
    Interpreter = 1 << 10,
    IoFault = 1 << 11,
    JointFault = 1 << 12,
    SpindleOrient = 1 << 13,
    MiscError = 1 << 14,
    InputTimeout = 1 << 15,
    HardLimit = 1 << 16,
    SoftLimit = 1 << 17,
    InvalidValue = 1 << 18,
    UnknownCode = 1 << 19,
    Transport = 1 << 20,
    StatusMessage = 1 << 21,
    ControllerFault = 1 << 22,
    SerialBridgeFault = 1 << 23,
    SpindleFault = 1 << 24,
    SpindleBlock = 1 << 25,
    DiagnosticInterface = 1 << 26,
}

impl DiagnosticCategory {
    pub const fn mask(self) -> u64 {
        self as u64
    }

    pub const fn classified_recovery(self) -> RecoveryClass {
        match self {
            Self::Transport => RecoveryClass::RecheckSource,
            Self::ControllerFault => RecoveryClass::ClearController,
            Self::SerialBridgeFault => RecoveryClass::RestorePendant,
            Self::HardLimit => RecoveryClass::ReleaseLimit,
            Self::TopRcs
            | Self::TaskRcs
            | Self::MotionRcs
            | Self::TrajectoryRcs
            | Self::JointRcs
            | Self::AxisRcs
            | Self::IoRcs
            | Self::TaskExec
            | Self::Interpreter
            | Self::InputTimeout
            | Self::SoftLimit
            | Self::StatusMessage => RecoveryClass::AbortTask,
            Self::IoFault | Self::JointFault | Self::MiscError => RecoveryClass::RestoreMachine,
            Self::SpindleRcs | Self::SpindleOrient | Self::SpindleFault | Self::SpindleBlock => {
                RecoveryClass::ResetSpindle
            }
            Self::Abi | Self::InvalidValue | Self::UnknownCode | Self::DiagnosticInterface => {
                RecoveryClass::RelaunchApplication
            }
        }
    }

    pub const fn operator_action(self) -> &'static str {
        match self {
            Self::Abi => {
                "stop this monitor and rebuild/reinstall it against the pinned LinuxCNC 2.9.10 interface"
            }
            Self::Transport => {
                "restore the LinuxCNC status-channel connection and confirm fresh valid status before resuming"
            }
            Self::UnknownCode => {
                "retain the raw domain/value, do not guess its meaning, and verify the running LinuxCNC source/version"
            }
            Self::HardLimit => {
                "inspect the named joint/axis and use only the configured limit recovery path after the physical cause is clear"
            }
            Self::SoftLimit => {
                "inspect the named axis position and commanded path, then correct the program or coordinate state"
            }
            Self::InputTimeout => {
                "inspect the named input and its configured timeout source before retrying"
            }
            Self::Interpreter => {
                "inspect the named interpreter result and correct the loaded program at its reported context"
            }
            Self::IoFault | Self::JointFault | Self::SpindleOrient => {
                "inspect the named source and retained LinuxCNC state, clear the physical/controller cause, then reset"
            }
            Self::InvalidValue => {
                "inspect the named source and correct it to one value from its documented domain"
            }
            Self::StatusMessage => {
                "read the exact LinuxCNC status message and correct its named source before continuing"
            }
            Self::TopRcs
            | Self::TaskRcs
            | Self::MotionRcs
            | Self::TrajectoryRcs
            | Self::JointRcs
            | Self::AxisRcs
            | Self::SpindleRcs
            | Self::IoRcs
            | Self::TaskExec
            | Self::MiscError => {
                "inspect the named LinuxCNC source, raw value, and cause; clear the underlying condition before continuing"
            }
            Self::ControllerFault => {
                "correct the named controller cause, then use Clear Fault and return to Pendant Mode"
            }
            Self::SerialBridgeFault => {
                "restore the Nano P3 stream, then use Clear Fault and return to Pendant Mode"
            }
            Self::SpindleFault | Self::SpindleBlock => {
                "stop the spindle request, correct the named drive cause, then use Clear Fault"
            }
            Self::DiagnosticInterface => {
                "retain the interface evidence and relaunch only after installing matching component binaries"
            }
        }
    }
}

impl RecoveryClassified for DiagnosticCategory {
    fn recovery_class(&self) -> RecoveryClass {
        self.classified_recovery()
    }
}

pub const ABI: DiagnosticCategory = DiagnosticCategory::Abi;
pub const TOP_RCS: DiagnosticCategory = DiagnosticCategory::TopRcs;
pub const TASK_RCS: DiagnosticCategory = DiagnosticCategory::TaskRcs;
pub const MOTION_RCS: DiagnosticCategory = DiagnosticCategory::MotionRcs;
pub const TRAJECTORY_RCS: DiagnosticCategory = DiagnosticCategory::TrajectoryRcs;
pub const JOINT_RCS: DiagnosticCategory = DiagnosticCategory::JointRcs;
pub const AXIS_RCS: DiagnosticCategory = DiagnosticCategory::AxisRcs;
pub const SPINDLE_RCS: DiagnosticCategory = DiagnosticCategory::SpindleRcs;
pub const IO_RCS: DiagnosticCategory = DiagnosticCategory::IoRcs;
pub const TASK_EXEC: DiagnosticCategory = DiagnosticCategory::TaskExec;
pub const INTERPRETER: DiagnosticCategory = DiagnosticCategory::Interpreter;
pub const IO_FAULT: DiagnosticCategory = DiagnosticCategory::IoFault;
pub const JOINT_FAULT: DiagnosticCategory = DiagnosticCategory::JointFault;
pub const SPINDLE_ORIENT: DiagnosticCategory = DiagnosticCategory::SpindleOrient;
pub const MISC_ERROR: DiagnosticCategory = DiagnosticCategory::MiscError;
pub const INPUT_TIMEOUT: DiagnosticCategory = DiagnosticCategory::InputTimeout;
pub const HARD_LIMIT: DiagnosticCategory = DiagnosticCategory::HardLimit;
pub const SOFT_LIMIT: DiagnosticCategory = DiagnosticCategory::SoftLimit;
pub const INVALID_VALUE: DiagnosticCategory = DiagnosticCategory::InvalidValue;
pub const UNKNOWN_CODE: DiagnosticCategory = DiagnosticCategory::UnknownCode;
pub const TRANSPORT: DiagnosticCategory = DiagnosticCategory::Transport;
pub const STATUS_MESSAGE: DiagnosticCategory = DiagnosticCategory::StatusMessage;
pub const CONTROLLER_FAULT: DiagnosticCategory = DiagnosticCategory::ControllerFault;
pub const SERIAL_BRIDGE_FAULT: DiagnosticCategory = DiagnosticCategory::SerialBridgeFault;
pub const SPINDLE_FAULT: DiagnosticCategory = DiagnosticCategory::SpindleFault;
pub const SPINDLE_BLOCK: DiagnosticCategory = DiagnosticCategory::SpindleBlock;
pub const DIAGNOSTIC_INTERFACE: DiagnosticCategory = DiagnosticCategory::DiagnosticInterface;
