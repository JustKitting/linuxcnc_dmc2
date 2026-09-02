use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::catalog::{Catalog, Operation, OperationKind};
use crate::native::{ControlBackend, MachineState, NativeError, Receipt, Status, TaskMode};

const STATUS_POLL_PERIOD: Duration = Duration::from_millis(20);

pub fn execute_control(
    operation: &Operation,
    backend: &mut impl ControlBackend,
    physical_estop_pressed: Option<bool>,
) -> Result<Receipt, DispatchError> {
    require_kind(operation, OperationKind::Control)?;
    let status = backend.status()?;
    check_prerequisites(operation, &status, physical_estop_pressed)?;
    match (operation.driver.as_str(), operation.target.as_str()) {
        ("linuxcnc.task-state", "estop") => {
            backend.set_state(MachineState::Estop).map_err(Into::into)
        }
        ("linuxcnc.task-state", "estop-reset") => backend
            .set_state(MachineState::EstopReset)
            .map_err(Into::into),
        ("linuxcnc.task-state", "on") => backend.set_state(MachineState::On).map_err(Into::into),
        ("linuxcnc.task-state", "off") => backend.set_state(MachineState::Off).map_err(Into::into),
        ("linuxcnc.abort", "-") => backend.abort().map_err(Into::into),
        ("linuxcnc.home-all", "-") => home_all(backend),
        _ => Err(DispatchError::UnsupportedDriver {
            id: operation.id.clone(),
            driver: operation.driver.clone(),
            target: operation.target.clone(),
        }),
    }
}

fn home_all(backend: &mut impl ControlBackend) -> Result<Receipt, DispatchError> {
    let status = backend.status()?;
    if status.task_mode != TaskMode::Manual {
        backend.set_manual_mode()?;
    }
    backend.set_teleop(false)?;
    let receipt = backend.home_all()?;
    loop {
        let status = backend.status()?;
        if status.all_homed() && status.homing_mask == 0 {
            return Ok(receipt);
        }
        if status.machine_state != MachineState::On || status.auxiliary_estop {
            return Err(DispatchError::HomingInterrupted {
                machine_state: status.machine_state,
                auxiliary_estop: status.auxiliary_estop,
                homed_mask: status.homed_mask,
                homing_mask: status.homing_mask,
            });
        }
        thread::sleep(STATUS_POLL_PERIOD);
    }
}

pub fn load_program(
    catalog: &Catalog,
    operation: &Operation,
    backend: &mut impl ControlBackend,
) -> Result<(PathBuf, Receipt), DispatchError> {
    require_kind(operation, OperationKind::Program)?;
    require_program_driver(operation)?;
    let path = program_path(catalog, operation)?;
    let status = backend.status()?;
    if !status.interpreter_idle() {
        return Err(DispatchError::LoadWhileInterpreterActive {
            interpreter_state: status.interpreter_state,
        });
    }

    backend.program_close()?;
    let receipt = backend.program_open(&path)?;
    let observed = backend.status()?;
    if !same_file(&path, &observed.loaded_file) {
        return Err(DispatchError::LoadedFileMismatch {
            expected: path,
            observed: observed.loaded_file,
        });
    }
    Ok((path, receipt))
}

pub fn run_program(
    catalog: &Catalog,
    operation: &Operation,
    backend: &mut impl ControlBackend,
) -> Result<Receipt, DispatchError> {
    require_kind(operation, OperationKind::Program)?;
    require_program_driver(operation)?;
    let path = program_path(catalog, operation)?;
    let status = backend.status()?;
    if !same_file(&path, &status.loaded_file) {
        return Err(DispatchError::RunRequiresExactLoadedProgram {
            requested: path,
            loaded: status.loaded_file,
        });
    }
    check_prerequisites(operation, &status, None)?;
    if status.task_mode != TaskMode::Auto {
        backend.set_auto_mode()?;
    }
    backend.program_run().map_err(Into::into)
}

fn require_kind(operation: &Operation, expected: OperationKind) -> Result<(), DispatchError> {
    if operation.kind == expected {
        Ok(())
    } else {
        Err(DispatchError::WrongKind {
            id: operation.id.clone(),
            expected,
            observed: operation.kind,
        })
    }
}

fn require_program_driver(operation: &Operation) -> Result<(), DispatchError> {
    if operation.driver == "linuxcnc.program" {
        Ok(())
    } else {
        Err(DispatchError::UnsupportedDriver {
            id: operation.id.clone(),
            driver: operation.driver.clone(),
            target: operation.target.clone(),
        })
    }
}

fn program_path(catalog: &Catalog, operation: &Operation) -> Result<PathBuf, DispatchError> {
    let project_root =
        fs::canonicalize(catalog.project_root()).map_err(|source| DispatchError::ProgramPath {
            id: operation.id.clone(),
            path: catalog.project_root().to_path_buf(),
            source,
        })?;
    let requested = catalog.project_root().join(&operation.target);
    let path = fs::canonicalize(&requested).map_err(|source| DispatchError::ProgramPath {
        id: operation.id.clone(),
        path: requested,
        source,
    })?;
    if !path.starts_with(&project_root) {
        return Err(DispatchError::ProgramOutsideProject {
            id: operation.id.clone(),
            path,
            project_root,
        });
    }
    Ok(path)
}

fn same_file(expected: &Path, observed: &Path) -> bool {
    if observed.as_os_str().is_empty() {
        return false;
    }
    match (fs::canonicalize(expected), fs::canonicalize(observed)) {
        (Ok(expected), Ok(observed)) => expected == observed,
        _ => expected == observed,
    }
}

fn check_prerequisites(
    operation: &Operation,
    status: &Status,
    physical_estop_pressed: Option<bool>,
) -> Result<(), DispatchError> {
    for prerequisite in &operation.prerequisites {
        let satisfied = match prerequisite.as_str() {
            "running-session" => true,
            "physical-estop-released" => physical_estop_pressed == Some(false),
            "estop-clear" => status.machine_state != MachineState::Estop && !status.auxiliary_estop,
            "machine-on" => status.machine_state == MachineState::On,
            "interpreter-idle" => status.interpreter_idle(),
            "all-homed" => status.all_homed(),
            unknown => {
                return Err(DispatchError::UnknownPrerequisite {
                    id: operation.id.clone(),
                    prerequisite: unknown.to_owned(),
                })
            }
        };
        if !satisfied {
            return Err(DispatchError::Prerequisite {
                id: operation.id.clone(),
                prerequisite: prerequisite.clone(),
                machine_state: status.machine_state,
                task_mode: status.task_mode,
                interpreter_state: status.interpreter_state,
                homed_mask: status.homed_mask,
                joint_count: status.joint_count,
                physical_estop_pressed,
            });
        }
    }
    Ok(())
}

#[derive(Debug)]
pub enum DispatchError {
    Native(NativeError),
    WrongKind {
        id: String,
        expected: OperationKind,
        observed: OperationKind,
    },
    UnsupportedDriver {
        id: String,
        driver: String,
        target: String,
    },
    UnknownPrerequisite {
        id: String,
        prerequisite: String,
    },
    Prerequisite {
        id: String,
        prerequisite: String,
        machine_state: MachineState,
        task_mode: TaskMode,
        interpreter_state: i32,
        homed_mask: u32,
        joint_count: i32,
        physical_estop_pressed: Option<bool>,
    },
    ProgramPath {
        id: String,
        path: PathBuf,
        source: std::io::Error,
    },
    ProgramOutsideProject {
        id: String,
        path: PathBuf,
        project_root: PathBuf,
    },
    LoadWhileInterpreterActive {
        interpreter_state: i32,
    },
    LoadedFileMismatch {
        expected: PathBuf,
        observed: PathBuf,
    },
    RunRequiresExactLoadedProgram {
        requested: PathBuf,
        loaded: PathBuf,
    },
    HomingInterrupted {
        machine_state: MachineState,
        auxiliary_estop: bool,
        homed_mask: u32,
        homing_mask: u32,
    },
}

impl From<NativeError> for DispatchError {
    fn from(value: NativeError) -> Self {
        Self::Native(value)
    }
}

impl fmt::Display for DispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(error) => error.fmt(formatter),
            Self::WrongKind {
                id,
                expected,
                observed,
            } => write!(
                formatter,
                "operation {id} has kind {}; expected {}",
                observed.name(),
                expected.name()
            ),
            Self::UnsupportedDriver { id, driver, target } => write!(
                formatter,
                "operation {id} uses unsupported driver={driver:?} target={target:?}"
            ),
            Self::UnknownPrerequisite { id, prerequisite } => write!(
                formatter,
                "operation {id} has unknown prerequisite {prerequisite:?}"
            ),
            Self::Prerequisite {
                id,
                prerequisite,
                machine_state,
                task_mode,
                interpreter_state,
                homed_mask,
                joint_count,
                physical_estop_pressed,
            } => write!(
                formatter,
                "OPERATION_PREREQUISITE_FAILED: operation={id} prerequisite={prerequisite} machine_state={} task_mode={} interpreter_state={interpreter_state} homed_mask=0x{homed_mask:08x} joint_count={joint_count} physical_estop_pressed={physical_estop_pressed:?}",
                machine_state.name(),
                task_mode.name()
            ),
            Self::ProgramPath { id, path, source } => write!(
                formatter,
                "program path for {id} is unavailable at {}: {source}",
                path.display()
            ),
            Self::ProgramOutsideProject {
                id,
                path,
                project_root,
            } => write!(
                formatter,
                "program {id} resolves outside project: path={} root={}",
                path.display(),
                project_root.display()
            ),
            Self::LoadWhileInterpreterActive { interpreter_state } => write!(
                formatter,
                "PROGRAM_LOAD_BLOCKED: interpreter_state={interpreter_state}; abort or wait for idle"
            ),
            Self::LoadedFileMismatch { expected, observed } => write!(
                formatter,
                "PROGRAM_LOAD_NOT_ACKNOWLEDGED: expected={} observed={}",
                expected.display(),
                observed.display()
            ),
            Self::RunRequiresExactLoadedProgram { requested, loaded } => write!(
                formatter,
                "PROGRAM_RUN_REQUIRES_EXACT_LOAD: requested={} loaded={}; run dmc2ctl load first",
                requested.display(),
                loaded.display()
            ),
            Self::HomingInterrupted {
                machine_state,
                auxiliary_estop,
                homed_mask,
                homing_mask,
            } => write!(
                formatter,
                "HOMING_INTERRUPTED: machine_state={} auxiliary_estop={} homed_mask=0x{homed_mask:08x} homing_mask=0x{homing_mask:08x}",
                machine_state.name(),
                auxiliary_estop
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{default_catalog_path, Catalog};
    use crate::native::{NativeOperation, Receipt};

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum ObservedAction {
        SetAuto,
        Close,
        Open(PathBuf),
        Run,
    }

    struct FakeBackend {
        status: Status,
        actions: Vec<ObservedAction>,
    }

    impl ControlBackend for FakeBackend {
        fn status(&mut self) -> Result<Status, NativeError> {
            Ok(self.status.clone())
        }

        fn set_state(&mut self, state: MachineState) -> Result<Receipt, NativeError> {
            Err(fake_error(NativeOperation::SetState(state)))
        }

        fn set_manual_mode(&mut self) -> Result<Receipt, NativeError> {
            Err(fake_error(NativeOperation::SetManualMode))
        }

        fn set_auto_mode(&mut self) -> Result<Receipt, NativeError> {
            self.actions.push(ObservedAction::SetAuto);
            self.status.task_mode = TaskMode::Auto;
            Ok(Receipt::default())
        }

        fn abort(&mut self) -> Result<Receipt, NativeError> {
            Err(fake_error(NativeOperation::Abort))
        }

        fn set_teleop(&mut self, enabled: bool) -> Result<Receipt, NativeError> {
            Err(fake_error(NativeOperation::SetTeleop(enabled)))
        }

        fn home_all(&mut self) -> Result<Receipt, NativeError> {
            Err(fake_error(NativeOperation::HomeAll))
        }

        fn program_close(&mut self) -> Result<Receipt, NativeError> {
            self.actions.push(ObservedAction::Close);
            Ok(Receipt::default())
        }

        fn program_open(&mut self, path: &Path) -> Result<Receipt, NativeError> {
            self.actions.push(ObservedAction::Open(path.to_path_buf()));
            self.status.loaded_file = path.to_path_buf();
            Ok(Receipt::default())
        }

        fn program_run(&mut self) -> Result<Receipt, NativeError> {
            self.actions.push(ObservedAction::Run);
            Ok(Receipt::default())
        }
    }

    fn fake_error(operation: NativeOperation) -> NativeError {
        NativeError::Command {
            operation,
            result: 6,
            receipt: Receipt::default(),
        }
    }

    fn ready_status() -> Status {
        Status {
            machine_state: MachineState::On,
            task_mode: TaskMode::Manual,
            interpreter_state: 1,
            execution_state: 2,
            rcs_status: 1,
            echo_serial_number: 0,
            joint_count: 3,
            homed_mask: 0b111,
            homing_mask: 0,
            position: [0.0; 3],
            spindle_speed: 0.0,
            spindle_direction: 0,
            auxiliary_estop: false,
            loaded_file: PathBuf::new(),
        }
    }

    #[test]
    fn load_dispatch_cannot_emit_run() {
        let catalog = Catalog::open(default_catalog_path()).expect("catalog should parse");
        let operation = catalog
            .operation("program.puck-contact-no-motion")
            .expect("program should exist");
        let mut backend = FakeBackend {
            status: ready_status(),
            actions: Vec::new(),
        };

        load_program(&catalog, operation, &mut backend).expect("load should be acknowledged");

        assert!(matches!(
            backend.actions.as_slice(),
            [ObservedAction::Close, ObservedAction::Open(_)]
        ));
    }

    #[test]
    fn run_dispatch_cannot_open_a_program() {
        let catalog = Catalog::open(default_catalog_path()).expect("catalog should parse");
        let operation = catalog
            .operation("program.puck-contact-no-motion")
            .expect("program should exist");
        let loaded = program_path(&catalog, operation).expect("program should resolve");
        let mut status = ready_status();
        status.loaded_file = loaded;
        let mut backend = FakeBackend {
            status,
            actions: Vec::new(),
        };

        run_program(&catalog, operation, &mut backend).expect("run should be accepted");

        assert_eq!(
            backend.actions,
            [ObservedAction::SetAuto, ObservedAction::Run]
        );
    }
}
