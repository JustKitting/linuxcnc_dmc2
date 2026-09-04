use std::ffi::{CString, OsString};
use std::fmt;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};

#[allow(
    dead_code,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals
)]
mod ffi {
    include!(concat!(env!("OUT_DIR"), "/control_client_bindings.rs"));
}

const TASK_STATE_ESTOP: i32 = 1;
const TASK_STATE_ESTOP_RESET: i32 = 2;
const TASK_STATE_OFF: i32 = 3;
const TASK_STATE_ON: i32 = 4;
const TASK_MODE_MANUAL: i32 = 1;
const TASK_MODE_AUTO: i32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MachineState {
    Estop,
    EstopReset,
    Off,
    On,
    Unknown(i32),
}

impl MachineState {
    fn from_raw(value: i32) -> Self {
        match value {
            TASK_STATE_ESTOP => Self::Estop,
            TASK_STATE_ESTOP_RESET => Self::EstopReset,
            TASK_STATE_OFF => Self::Off,
            TASK_STATE_ON => Self::On,
            other => Self::Unknown(other),
        }
    }

    fn raw(self) -> Option<i32> {
        match self {
            Self::Estop => Some(TASK_STATE_ESTOP),
            Self::EstopReset => Some(TASK_STATE_ESTOP_RESET),
            Self::Off => Some(TASK_STATE_OFF),
            Self::On => Some(TASK_STATE_ON),
            Self::Unknown(_) => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Estop => "estop",
            Self::EstopReset => "estop-reset",
            Self::Off => "off",
            Self::On => "on",
            Self::Unknown(_) => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskMode {
    Manual,
    Auto,
    Mdi,
    Unknown(i32),
}

impl TaskMode {
    fn from_raw(value: i32) -> Self {
        match value {
            1 => Self::Manual,
            2 => Self::Auto,
            3 => Self::Mdi,
            other => Self::Unknown(other),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
            Self::Mdi => "mdi",
            Self::Unknown(_) => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub machine_state: MachineState,
    pub task_mode: TaskMode,
    pub interpreter_state: i32,
    pub execution_state: i32,
    pub rcs_status: i32,
    pub echo_serial_number: i32,
    pub joint_count: i32,
    pub homed_mask: u32,
    pub homing_mask: u32,
    pub position: [f64; 3],
    pub spindle_speed: f64,
    pub spindle_direction: i32,
    pub auxiliary_estop: bool,
    pub loaded_file: PathBuf,
}

impl Status {
    pub const fn interpreter_idle(&self) -> bool {
        self.interpreter_state == 1
    }

    pub fn all_homed(&self) -> bool {
        if self.joint_count <= 0 || self.joint_count > 32 {
            return false;
        }
        let expected = if self.joint_count == 32 {
            u32::MAX
        } else {
            (1_u32 << self.joint_count) - 1
        };
        self.homed_mask & expected == expected
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Receipt {
    pub command_serial_number: i32,
    pub echo_serial_number: i32,
    pub rcs_status: i32,
    pub nml_error: i32,
    pub cms_status: i32,
}

impl From<ffi::dmc2_command_receipt> for Receipt {
    fn from(value: ffi::dmc2_command_receipt) -> Self {
        Self {
            command_serial_number: value.command_serial_number,
            echo_serial_number: value.echo_serial_number,
            rcs_status: value.rcs_status,
            nml_error: value.nml_error,
            cms_status: value.cms_status,
        }
    }
}

pub trait ControlBackend {
    fn status(&mut self) -> Result<Status, NativeError>;
    fn set_state(&mut self, state: MachineState) -> Result<Receipt, NativeError>;
    fn set_manual_mode(&mut self) -> Result<Receipt, NativeError>;
    fn set_auto_mode(&mut self) -> Result<Receipt, NativeError>;
    fn abort(&mut self) -> Result<Receipt, NativeError>;
    fn set_teleop(&mut self, enabled: bool) -> Result<Receipt, NativeError>;
    fn home_all(&mut self) -> Result<Receipt, NativeError>;
    fn program_close(&mut self) -> Result<Receipt, NativeError>;
    fn program_open(&mut self, path: &Path) -> Result<Receipt, NativeError>;
    fn program_run(&mut self) -> Result<Receipt, NativeError>;
}

pub struct Session {
    native: *mut ffi::dmc2_control_session,
}

impl Session {
    pub fn open(path: &Path) -> Result<Self, NativeError> {
        let native_version = unsafe { ffi::dmc2_control_abi_version() };
        let native_size = unsafe { ffi::dmc2_control_status_size() };
        if native_version != ffi::DMC2_CONTROL_ABI_VERSION
            || native_size != std::mem::size_of::<ffi::dmc2_control_status>()
        {
            return Err(NativeError::AbiMismatch {
                native_version,
                native_size,
                rust_version: ffi::DMC2_CONTROL_ABI_VERSION,
                rust_size: std::mem::size_of::<ffi::dmc2_control_status>(),
            });
        }
        let path_c = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
            NativeError::PathContainsNul {
                path: path.to_path_buf(),
            }
        })?;
        let mut diagnostic = ffi::dmc2_command_receipt::default();
        let native = unsafe { ffi::dmc2_control_open(path_c.as_ptr(), &mut diagnostic) };
        if native.is_null() {
            return Err(NativeError::Open {
                nml_file: path.to_path_buf(),
                receipt: diagnostic.into(),
            });
        }
        Ok(Self { native })
    }

    fn command(
        &mut self,
        operation: NativeOperation,
        invoke: impl FnOnce(
            *mut ffi::dmc2_control_session,
            *mut ffi::dmc2_command_receipt,
        ) -> ffi::dmc2_control_result,
    ) -> Result<Receipt, NativeError> {
        let mut receipt = ffi::dmc2_command_receipt::default();
        let result = invoke(self.native, &mut receipt);
        if result == ffi::DMC2_CONTROL_OK {
            Ok(receipt.into())
        } else {
            Err(NativeError::Command {
                operation,
                result,
                receipt: receipt.into(),
            })
        }
    }
}

impl ControlBackend for Session {
    fn status(&mut self) -> Result<Status, NativeError> {
        let mut raw = ffi::dmc2_control_status::default();
        let mut diagnostic = ffi::dmc2_command_receipt::default();
        let result =
            unsafe { ffi::dmc2_control_read_status(self.native, &mut raw, &mut diagnostic) };
        if result != ffi::DMC2_CONTROL_OK {
            return Err(NativeError::Status {
                result,
                receipt: diagnostic.into(),
            });
        }
        if raw.abi_version != ffi::DMC2_CONTROL_ABI_VERSION
            || raw.struct_size as usize != std::mem::size_of::<ffi::dmc2_control_status>()
        {
            return Err(NativeError::SnapshotAbi {
                version: raw.abi_version,
                size: raw.struct_size,
            });
        }
        let file_end = raw
            .loaded_file
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(raw.loaded_file.len());
        let loaded_file = PathBuf::from(OsString::from_vec(raw.loaded_file[..file_end].to_vec()));
        Ok(Status {
            machine_state: MachineState::from_raw(raw.task_state),
            task_mode: TaskMode::from_raw(raw.task_mode),
            interpreter_state: raw.interpreter_state,
            execution_state: raw.execution_state,
            rcs_status: raw.rcs_status,
            echo_serial_number: raw.echo_serial_number,
            joint_count: raw.joint_count,
            homed_mask: raw.homed_mask,
            homing_mask: raw.homing_mask,
            position: [raw.position_x, raw.position_y, raw.position_z],
            spindle_speed: raw.spindle_speed,
            spindle_direction: raw.spindle_direction,
            auxiliary_estop: raw.auxiliary_estop != 0,
            loaded_file,
        })
    }

    fn set_state(&mut self, state: MachineState) -> Result<Receipt, NativeError> {
        let raw = state.raw().ok_or(NativeError::UnknownMachineState(state))?;
        self.command(
            NativeOperation::SetState(state),
            |session, receipt| unsafe { ffi::dmc2_control_set_state(session, raw, receipt) },
        )
    }

    fn set_manual_mode(&mut self) -> Result<Receipt, NativeError> {
        self.command(NativeOperation::SetManualMode, |session, receipt| unsafe {
            ffi::dmc2_control_set_mode(session, TASK_MODE_MANUAL, receipt)
        })
    }

    fn set_auto_mode(&mut self) -> Result<Receipt, NativeError> {
        self.command(NativeOperation::SetAutoMode, |session, receipt| unsafe {
            ffi::dmc2_control_set_mode(session, TASK_MODE_AUTO, receipt)
        })
    }

    fn abort(&mut self) -> Result<Receipt, NativeError> {
        self.command(NativeOperation::Abort, |session, receipt| unsafe {
            ffi::dmc2_control_abort(session, receipt)
        })
    }

    fn set_teleop(&mut self, enabled: bool) -> Result<Receipt, NativeError> {
        self.command(
            NativeOperation::SetTeleop(enabled),
            |session, receipt| unsafe {
                ffi::dmc2_control_set_teleop(session, i32::from(enabled), receipt)
            },
        )
    }

    fn home_all(&mut self) -> Result<Receipt, NativeError> {
        self.command(NativeOperation::HomeAll, |session, receipt| unsafe {
            ffi::dmc2_control_home(session, -1, receipt)
        })
    }

    fn program_close(&mut self) -> Result<Receipt, NativeError> {
        self.command(NativeOperation::ProgramClose, |session, receipt| unsafe {
            ffi::dmc2_control_program_close(session, receipt)
        })
    }

    fn program_open(&mut self, path: &Path) -> Result<Receipt, NativeError> {
        let path_c = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
            NativeError::PathContainsNul {
                path: path.to_path_buf(),
            }
        })?;
        self.command(NativeOperation::ProgramOpen, |session, receipt| unsafe {
            ffi::dmc2_control_program_open(session, path_c.as_ptr(), receipt)
        })
    }

    fn program_run(&mut self) -> Result<Receipt, NativeError> {
        self.command(NativeOperation::ProgramRun, |session, receipt| unsafe {
            ffi::dmc2_control_program_run(session, 0, receipt)
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe { ffi::dmc2_control_close(self.native) };
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeOperation {
    SetState(MachineState),
    SetManualMode,
    SetAutoMode,
    Abort,
    SetTeleop(bool),
    HomeAll,
    ProgramClose,
    ProgramOpen,
    ProgramRun,
}

impl NativeOperation {
    const fn failure_recovery_class(self) -> RecoveryClass {
        match self {
            Self::SetState(_) => RecoveryClass::RestoreMachine,
            Self::SetManualMode | Self::SetAutoMode | Self::SetTeleop(_) | Self::Abort => {
                RecoveryClass::RelaunchApplication
            }
            Self::HomeAll => RecoveryClass::EstablishPosition,
            Self::ProgramClose | Self::ProgramOpen | Self::ProgramRun => RecoveryClass::AbortTask,
        }
    }
}

#[derive(Debug)]
pub enum NativeError {
    AbiMismatch {
        native_version: u32,
        native_size: usize,
        rust_version: u32,
        rust_size: usize,
    },
    SnapshotAbi {
        version: u32,
        size: u32,
    },
    PathContainsNul {
        path: PathBuf,
    },
    Open {
        nml_file: PathBuf,
        receipt: Receipt,
    },
    Status {
        result: ffi::dmc2_control_result,
        receipt: Receipt,
    },
    UnknownMachineState(MachineState),
    Command {
        operation: NativeOperation,
        result: ffi::dmc2_control_result,
        receipt: Receipt,
    },
}

impl fmt::Display for NativeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AbiMismatch {
                native_version,
                native_size,
                rust_version,
                rust_size,
            } => write!(
                formatter,
                "CONTROL_ABI_MISMATCH: native=0x{native_version:08x}/{native_size} rust=0x{rust_version:08x}/{rust_size}"
            ),
            Self::SnapshotAbi { version, size } => write!(
                formatter,
                "CONTROL_SNAPSHOT_ABI_MISMATCH: version=0x{version:08x} size={size}"
            ),
            Self::PathContainsNul { path } => {
                write!(formatter, "path contains NUL: {}", path.display())
            }
            Self::Open { nml_file, receipt } => write!(
                formatter,
                "LINUXCNC_SESSION_OPEN_FAILED: nml={} nml_error={} cms_status={}",
                nml_file.display(),
                receipt.nml_error,
                receipt.cms_status
            ),
            Self::Status { result, receipt } => write!(
                formatter,
                "LINUXCNC_STATUS_FAILED: result={result} nml_error={} cms_status={}",
                receipt.nml_error, receipt.cms_status
            ),
            Self::UnknownMachineState(state) => {
                write!(formatter, "unknown machine state cannot be commanded: {state:?}")
            }
            Self::Command {
                operation,
                result,
                receipt,
            } => write!(
                formatter,
                "LINUXCNC_COMMAND_FAILED: operation={operation:?} result={result} command_serial={} echo_serial={} rcs_status={} nml_error={} cms_status={}",
                receipt.command_serial_number,
                receipt.echo_serial_number,
                receipt.rcs_status,
                receipt.nml_error,
                receipt.cms_status
            ),
        }
    }
}

impl RecoveryClassified for NativeError {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::AbiMismatch { .. }
            | Self::SnapshotAbi { .. }
            | Self::PathContainsNul { .. }
            | Self::Open { .. }
            | Self::Status { .. }
            | Self::UnknownMachineState(_) => RecoveryClass::RelaunchApplication,
            Self::Command { operation, .. } => operation.failure_recovery_class(),
        }
    }
}
