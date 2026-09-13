use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::catalog::{Catalog, Operation, OperationKind, Prerequisite};
use crate::native::{ControlBackend, MachineState, NativeError, Receipt, Status, TaskMode};
use crate::script::{ScriptContract, ScriptError};
use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};

pub(crate) const STATUS_POLL_PERIOD: Duration = Duration::from_millis(20);

#[derive(Clone, Copy)]
enum ProgramSource<'a> {
    Catalog(&'a Path),
    Inspected(&'a ScriptContract),
}

impl<'a> ProgramSource<'a> {
    fn path(self) -> &'a Path {
        match self {
            Self::Catalog(path) => path,
            Self::Inspected(script) => script.path(),
        }
    }

    fn revalidate(self) -> Result<(), DispatchError> {
        match self {
            Self::Catalog(_) => Ok(()),
            Self::Inspected(script) => script.revalidate().map_err(DispatchError::Script),
        }
    }
}

#[derive(Debug)]
pub enum ExecutionOutcome {
    Control(Receipt),
    Program {
        path: PathBuf,
        load: Receipt,
        run: Receipt,
    },
}

pub fn execute_operation(
    catalog: &Catalog,
    operation: &Operation,
    backend: &mut impl ControlBackend,
    physical_estop_pressed: Option<bool>,
) -> Result<ExecutionOutcome, DispatchError> {
    match operation.kind {
        OperationKind::Control => execute_control(operation, backend, physical_estop_pressed)
            .map(ExecutionOutcome::Control),
        OperationKind::Program => {
            require_program_driver(operation)?;
            let path = program_path(catalog, operation)?;
            execute_program_path(
                &operation.id,
                ProgramSource::Catalog(&path),
                &operation.prerequisites,
                backend,
            )
        }
        OperationKind::Ui | OperationKind::Internal => {
            Err(DispatchError::UnsupportedExecutionKind {
                id: operation.id.clone(),
                kind: operation.kind,
            })
        }
    }
}

pub fn execute_script(
    script: &ScriptContract,
    backend: &mut impl ControlBackend,
) -> Result<ExecutionOutcome, DispatchError> {
    execute_program_path(
        &script_identity(script.path()),
        ProgramSource::Inspected(script),
        script.prerequisites(),
        backend,
    )
}

pub fn execute_control(
    operation: &Operation,
    backend: &mut impl ControlBackend,
    physical_estop_pressed: Option<bool>,
) -> Result<Receipt, DispatchError> {
    if operation.driver == "dmc2.clear-fault" && operation.target == "mesa-and-estop-reset" {
        return crate::fault_clear::execute(backend).map_err(DispatchError::FaultClear);
    }
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
    load_program_path(&operation.id, ProgramSource::Catalog(&path), backend)
}

pub fn load_script(
    script: &ScriptContract,
    backend: &mut impl ControlBackend,
) -> Result<(PathBuf, Receipt), DispatchError> {
    load_program_path(
        &script_identity(script.path()),
        ProgramSource::Inspected(script),
        backend,
    )
}

fn load_program_path(
    identity: &str,
    source: ProgramSource<'_>,
    backend: &mut impl ControlBackend,
) -> Result<(PathBuf, Receipt), DispatchError> {
    let path = source.path();
    let status = backend.status()?;
    if !status.interpreter_idle() {
        return Err(DispatchError::LoadWhileInterpreterActive {
            identity: identity.to_owned(),
            path: path.to_path_buf(),
            interpreter_state: status.interpreter_state,
        });
    }

    source.revalidate()?;
    backend.program_close()?;
    source.revalidate()?;
    let receipt = backend.program_open(path)?;
    let observed = backend.status()?;
    if !same_file(path, &observed.loaded_file) {
        return Err(DispatchError::LoadedFileMismatch {
            expected: path.to_path_buf(),
            observed: observed.loaded_file,
        });
    }
    source.revalidate()?;
    Ok((path.to_path_buf(), receipt))
}

pub fn run_program(
    catalog: &Catalog,
    operation: &Operation,
    backend: &mut impl ControlBackend,
) -> Result<Receipt, DispatchError> {
    require_kind(operation, OperationKind::Program)?;
    require_program_driver(operation)?;
    let path = program_path(catalog, operation)?;
    run_program_path(
        &operation.id,
        ProgramSource::Catalog(&path),
        &operation.prerequisites,
        backend,
    )
}

fn execute_program_path(
    identity: &str,
    source: ProgramSource<'_>,
    prerequisites: &[Prerequisite],
    backend: &mut impl ControlBackend,
) -> Result<ExecutionOutcome, DispatchError> {
    let status = backend.status()?;
    check_prerequisite_values(identity, prerequisites, &status, None)?;
    let (path, load) = load_program_path(identity, source, backend)?;
    let run = run_program_path(identity, source, prerequisites, backend)?;
    Ok(ExecutionOutcome::Program { path, load, run })
}

fn run_program_path(
    identity: &str,
    source: ProgramSource<'_>,
    prerequisites: &[Prerequisite],
    backend: &mut impl ControlBackend,
) -> Result<Receipt, DispatchError> {
    let path = source.path();
    let status = backend.status()?;
    if !same_file(path, &status.loaded_file) {
        return Err(DispatchError::RunRequiresExactLoadedProgram {
            requested: path.to_path_buf(),
            loaded: status.loaded_file,
        });
    }
    check_prerequisite_values(identity, prerequisites, &status, None)?;
    source.revalidate()?;
    if status.task_mode != TaskMode::Auto {
        backend.set_auto_mode()?;
    }
    // Mode acknowledgement can wait. Recheck both state and content after
    // that wait, immediately before issuing the explicitly requested Run.
    let status = backend.status()?;
    if !same_file(path, &status.loaded_file) {
        return Err(DispatchError::RunRequiresExactLoadedProgram {
            requested: path.to_path_buf(),
            loaded: status.loaded_file,
        });
    }
    check_prerequisite_values(identity, prerequisites, &status, None)?;
    source.revalidate()?;
    backend.program_run().map_err(Into::into)
}

fn script_identity(path: &Path) -> String {
    format!("file:{}", path.display())
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
    check_prerequisite_values(
        &operation.id,
        &operation.prerequisites,
        status,
        physical_estop_pressed,
    )
}

fn check_prerequisite_values(
    identity: &str,
    prerequisites: &[Prerequisite],
    status: &Status,
    physical_estop_pressed: Option<bool>,
) -> Result<(), DispatchError> {
    for prerequisite in prerequisites {
        let satisfied = match prerequisite {
            // This backend operates an already-running LinuxCNC session. UI
            // launch operations are rejected by `require_kind` before this
            // function and cannot manufacture desktop-session evidence here.
            Prerequisite::DesktopSession => false,
            Prerequisite::RunningSession => true,
            Prerequisite::PhysicalEstopReleased => physical_estop_pressed == Some(false),
            Prerequisite::EstopClear => {
                status.machine_state != MachineState::Estop && !status.auxiliary_estop
            }
            Prerequisite::MachineOn => status.machine_state == MachineState::On,
            Prerequisite::InterpreterIdle => status.interpreter_idle(),
            Prerequisite::AllHomed => status.all_homed(),
        };
        if !satisfied {
            return Err(DispatchError::Prerequisite {
                id: identity.to_owned(),
                prerequisite: *prerequisite,
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
    FaultClear(crate::fault_clear::FaultClearError),
    Script(ScriptError),
    Native(NativeError),
    WrongKind {
        id: String,
        expected: OperationKind,
        observed: OperationKind,
    },
    UnsupportedExecutionKind {
        id: String,
        kind: OperationKind,
    },
    UnsupportedDriver {
        id: String,
        driver: String,
        target: String,
    },
    Prerequisite {
        id: String,
        prerequisite: Prerequisite,
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
        identity: String,
        path: PathBuf,
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
            Self::FaultClear(error) => error.fmt(formatter),
            Self::Script(error) => error.fmt(formatter),
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
            Self::UnsupportedExecutionKind { id, kind } => write!(
                formatter,
                "operation {id} has kind {}; execute accepts only control or program operations",
                kind.name()
            ),
            Self::UnsupportedDriver { id, driver, target } => write!(
                formatter,
                "operation {id} uses unsupported driver={driver:?} target={target:?}"
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
                "OPERATION_PREREQUISITE_FAILED: operation={id} prerequisite={} machine_state={} task_mode={} interpreter_state={interpreter_state} homed_mask=0x{homed_mask:08x} joint_count={joint_count} physical_estop_pressed={physical_estop_pressed:?}",
                prerequisite.name(),
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
            Self::LoadWhileInterpreterActive {
                identity,
                path,
                interpreter_state,
            } => write!(
                formatter,
                "PROGRAM_LOAD_BLOCKED: operation={identity} file={} interpreter_state={interpreter_state}; action: use the visible Abort control or wait for the current task to become idle, then retry",
                path.display()
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

impl RecoveryClassified for DispatchError {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::FaultClear(error) => error.recovery_class(),
            Self::Script(error) => error.recovery_class(),
            Self::Native(error) => error.recovery_class(),
            Self::Prerequisite { prerequisite, .. } => prerequisite.recovery_class(),
            Self::WrongKind { .. }
            | Self::UnsupportedExecutionKind { .. }
            | Self::UnsupportedDriver { .. }
            | Self::ProgramPath { .. }
            | Self::ProgramOutsideProject { .. } => RecoveryClass::RelaunchApplication,
            Self::LoadWhileInterpreterActive { .. }
            | Self::LoadedFileMismatch { .. }
            | Self::RunRequiresExactLoadedProgram { .. } => RecoveryClass::AbortTask,
            Self::HomingInterrupted { .. } => RecoveryClass::EstablishPosition,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{default_catalog_path, Catalog};
    use crate::native::{CommandFailure, NativeOperation, Receipt};

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
        replace_on: Option<(ObservedAction, PathBuf)>,
    }

    impl FakeBackend {
        fn record(&mut self, action: ObservedAction) {
            if let Some((trigger, path)) = &self.replace_on {
                if *trigger == action {
                    fs::write(path, b"(changed after inspection)\nM2\n").unwrap();
                }
            }
            self.actions.push(action);
        }
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
            self.record(ObservedAction::SetAuto);
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
            self.record(ObservedAction::Close);
            Ok(Receipt::default())
        }

        fn program_open(&mut self, path: &Path) -> Result<Receipt, NativeError> {
            self.record(ObservedAction::Open(path.to_path_buf()));
            self.status.loaded_file = path.to_path_buf();
            Ok(Receipt::default())
        }

        fn program_run(&mut self) -> Result<Receipt, NativeError> {
            self.record(ObservedAction::Run);
            Ok(Receipt::default())
        }
    }

    fn fake_error(operation: NativeOperation) -> NativeError {
        NativeError::Command {
            operation,
            result: CommandFailure::Rejected,
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
    fn changed_script_never_reaches_run_across_load_and_mode_waits() {
        use std::io::Write;
        for stage in 0..4 {
            let path = std::env::temp_dir().join(format!(
                "dmc2-script-revision-{}-{stage}.ngc",
                std::process::id()
            ));
            let mut file = fs::File::options()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap();
            file.write_all(b"M2\n").unwrap();
            drop(file);
            let script = ScriptContract::open(&path).unwrap();
            let trigger = match stage {
                0 => {
                    fs::write(&path, b"(changed before dispatch)\nM2\n").unwrap();
                    None
                }
                1 => Some(ObservedAction::Close),
                2 => Some(ObservedAction::Open(script.path().to_path_buf())),
                3 => Some(ObservedAction::SetAuto),
                _ => unreachable!(),
            };
            let mut backend = FakeBackend {
                status: ready_status(),
                actions: Vec::new(),
                replace_on: trigger.map(|action| (action, path.clone())),
            };
            let result = execute_script(&script, &mut backend);
            fs::remove_file(&path).unwrap();
            assert!(matches!(
                result,
                Err(DispatchError::Script(ScriptError::ContentChanged { .. }))
            ));
            assert!(!backend.actions.contains(&ObservedAction::Run));
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
            replace_on: None,
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
            replace_on: None,
        };

        run_program(&catalog, operation, &mut backend).expect("run should be accepted");

        assert_eq!(
            backend.actions,
            [ObservedAction::SetAuto, ObservedAction::Run]
        );
    }

    #[test]
    fn execute_program_dispatches_one_cataloged_load_and_run_sequence() {
        let catalog = Catalog::open(default_catalog_path()).expect("catalog should parse");
        let operation = catalog
            .operation("program.puck-contact-no-motion")
            .expect("program should exist");
        let mut backend = FakeBackend {
            status: ready_status(),
            actions: Vec::new(),
            replace_on: None,
        };

        let outcome = execute_operation(&catalog, operation, &mut backend, None)
            .expect("cataloged program should dispatch");

        assert!(matches!(outcome, ExecutionOutcome::Program { .. }));
        assert!(matches!(
            backend.actions.as_slice(),
            [
                ObservedAction::Close,
                ObservedAction::Open(_),
                ObservedAction::SetAuto,
                ObservedAction::Run
            ]
        ));
    }
}
