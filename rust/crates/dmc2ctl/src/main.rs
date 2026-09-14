mod catalog;
mod cli;
mod dispatch;
mod fault_clear;
mod hal;
mod native;
mod script;

use std::fmt;
use std::fmt::Write as _;
use std::os::unix::ffi::OsStrExt;

use dmc2_diagnostics::{RecoveryClass, RecoveryClassified, RecoveryDisplay};

use catalog::{Catalog, CatalogError, Operation, OperationKind, Prerequisite};
use cli::{Action, CliError};
use dispatch::{DispatchError, ExecutionOutcome};
use hal::HalError;
use native::{ControlBackend, NativeError, Session, Status};
use script::{ScriptContract, ScriptError, INSPECTION_FORMAT};

fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.first().and_then(|v| v.to_str()) == Some("object-map") {
        // Offline object data commands never open NML or publish machine faults.
        match dmc2ctl::object_map::cli::run(&args[1..], &dmc2ctl::object_map::cli::default_store())
        {
            Ok(output) => println!("{output}"),
            Err(error) => {
                eprintln!("dmc2ctl object-map: {error}");
                std::process::exit(1);
            }
        }
        return;
    }
    match run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("dmc2ctl: {}", RecoveryDisplay(&error));
            if matches!(error, ApplicationError::Cli(_)) {
                eprintln!("{}", cli::USAGE);
            }
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), ApplicationError> {
    let Some(arguments) = cli::parse_environment()? else {
        println!("{}", cli::USAGE);
        return Ok(());
    };
    let catalog_path = arguments.catalog;
    let nml_file = arguments.nml_file;
    match arguments.action {
        Action::List => print_operations(&Catalog::open(catalog_path)?),
        Action::Describe(id) => {
            let catalog = Catalog::open(catalog_path)?;
            print_operation(catalog.operation(&id)?);
        }
        Action::Status => {
            let mut session = Session::open(&nml_file)?;
            print_status(&session.status()?);
        }
        Action::ClearFault => {
            // This built-in bypasses the catalog and submits to realtime before
            // opening task communication. Neither can veto the clear request.
            let request = fault_clear::submit().map_err(DispatchError::FaultClear)?;
            let mut session = Session::open(&nml_file)
                .map_err(|error| DispatchError::FaultClear(error.into()))?;
            let receipt = fault_clear::execute_requested(&mut session, request)
                .map_err(DispatchError::FaultClear)?;
            print_execution(
                "controller.clear-fault",
                &ExecutionOutcome::Control(receipt),
            );
        }
        Action::Execute(id) => {
            let catalog = Catalog::open(catalog_path)?;
            let operation = catalog.operation(&id)?;
            let physical_estop_pressed = if operation
                .prerequisites
                .iter()
                .any(|value| *value == Prerequisite::PhysicalEstopReleased)
            {
                Some(hal::read_bit(hal::PHYSICAL_PENDANT_ESTOP_PIN)?)
            } else {
                None
            };
            let mut session = Session::open(&nml_file)?;
            let outcome = dispatch::execute_operation(
                &catalog,
                operation,
                &mut session,
                physical_estop_pressed,
            )?;
            print_execution(&operation.id, &outcome);
        }
        Action::Load(id) => {
            let catalog = Catalog::open(catalog_path)?;
            let operation = catalog.operation(&id)?;
            let mut session = Session::open(&nml_file)?;
            let (path, receipt) = dispatch::load_program(&catalog, operation, &mut session)?;
            println!(
                "operation={} action=load file={} command_serial={} echo_serial={} rcs_status={}",
                operation.id,
                path.display(),
                receipt.command_serial_number,
                receipt.echo_serial_number,
                receipt.rcs_status
            );
        }
        Action::Run(id) => {
            let catalog = Catalog::open(catalog_path)?;
            let operation = catalog.operation(&id)?;
            let mut session = Session::open(&nml_file)?;
            let receipt = dispatch::run_program(&catalog, operation, &mut session)?;
            println!(
                "operation={} action=run command_serial={} echo_serial={} rcs_status={}",
                operation.id,
                receipt.command_serial_number,
                receipt.echo_serial_number,
                receipt.rcs_status
            );
        }
        Action::InspectFile(path) => {
            let script = ScriptContract::open(&path)?;
            print_script_contract(&script);
        }
        Action::LoadFile(path) => {
            let script = ScriptContract::open(&path)?;
            let mut session = Session::open(&nml_file)?;
            let (loaded, receipt) = dispatch::load_script(&script, &mut session)?;
            println!(
                "operation=file:{} action=load-file file={} contract_source={} command_serial={} echo_serial={} rcs_status={}",
                loaded.display(),
                loaded.display(),
                script.source().name(),
                receipt.command_serial_number,
                receipt.echo_serial_number,
                receipt.rcs_status
            );
        }
        Action::ExecuteFile(path) => {
            let script = ScriptContract::open(&path)?;
            let mut session = Session::open(&nml_file)?;
            let outcome = dispatch::execute_script(&script, &mut session)?;
            print_execution(&format!("file:{}", script.path().display()), &outcome);
        }
    }
    Ok(())
}

fn print_operations(catalog: &Catalog) {
    println!("catalog={}", catalog.path().display());
    println!("id\tkind\tlabel\tdriver\teffects\tprerequisites\tui_target");
    for operation in catalog
        .operations()
        .filter(|operation| operation.kind != OperationKind::Internal)
    {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            operation.id,
            operation.kind.name(),
            operation.label,
            operation.driver,
            operation.effects.join(";"),
            operation
                .prerequisites
                .iter()
                .map(|value| value.name())
                .collect::<Vec<_>>()
                .join(";"),
            operation.ui_target,
        );
    }
}

fn print_operation(operation: &Operation) {
    println!("id={}", operation.id);
    println!("kind={}", operation.kind.name());
    println!("label={}", operation.label);
    println!("driver={}", operation.driver);
    println!("target={}", operation.target);
    println!("ui_scope={}", operation.ui_scope);
    println!("ui_target={}", operation.ui_target);
    println!("effects={}", operation.effects.join(";"));
    println!(
        "prerequisites={}",
        operation
            .prerequisites
            .iter()
            .map(|value| value.name())
            .collect::<Vec<_>>()
            .join(";")
    );
}

fn print_status(status: &Status) {
    println!("machine_state={}", status.machine_state.name());
    println!("task_mode={}", status.task_mode.name());
    println!("interpreter_state={}", status.interpreter_state);
    println!("execution_state={}", status.execution_state);
    println!("rcs_status={}", status.rcs_status);
    println!("echo_serial={}", status.echo_serial_number);
    println!("joint_count={}", status.joint_count);
    println!("homed_mask=0x{:08x}", status.homed_mask);
    println!("homing_mask=0x{:08x}", status.homing_mask);
    println!("position_x={:.6}", status.position[0]);
    println!("position_y={:.6}", status.position[1]);
    println!("position_z={:.6}", status.position[2]);
    println!("spindle_speed={:.3}", status.spindle_speed);
    println!("spindle_direction={}", status.spindle_direction);
    println!("auxiliary_estop={}", status.auxiliary_estop);
    println!("loaded_file={}", status.loaded_file.display());
}

fn print_execution(identity: &str, outcome: &ExecutionOutcome) {
    match outcome {
        ExecutionOutcome::Control(receipt) => println!(
            "operation={} action=execute command_serial={} echo_serial={} rcs_status={}",
            identity,
            receipt.command_serial_number,
            receipt.echo_serial_number,
            receipt.rcs_status
        ),
        ExecutionOutcome::Program { path, load, run } => println!(
            "operation={} action=execute-program file={} load_command_serial={} load_echo_serial={} load_rcs_status={} run_command_serial={} run_echo_serial={} run_rcs_status={}",
            identity,
            path.display(),
            load.command_serial_number,
            load.echo_serial_number,
            load.rcs_status,
            run.command_serial_number,
            run.echo_serial_number,
            run.rcs_status
        ),
    }
}

fn print_script_contract(script: &ScriptContract) {
    println!("format={INSPECTION_FORMAT}");
    println!("path_hex={}", hex(script.path().as_os_str().as_bytes()));
    println!("content_bytes={}", script.revision().bytes());
    println!("content_fnv1a64={:016x}", script.revision().fnv1a64());
    println!("contract_source={}", script.source().name());
    println!(
        "effects={}",
        script
            .effects()
            .iter()
            .map(|effect| effect.name())
            .collect::<Vec<_>>()
            .join(";")
    );
    println!(
        "prerequisites={}",
        script
            .prerequisites()
            .iter()
            .map(|prerequisite| prerequisite.name())
            .collect::<Vec<_>>()
            .join(";")
    );
    println!("recovery_class={}", script.recovery().name());
    println!("recovery_slug={}", script.recovery().hal_slug());
}

fn hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

#[derive(Debug)]
enum ApplicationError {
    Cli(CliError),
    Catalog(CatalogError),
    Native(NativeError),
    Dispatch(DispatchError),
    Hal(HalError),
    Script(ScriptError),
}

impl From<CliError> for ApplicationError {
    fn from(value: CliError) -> Self {
        Self::Cli(value)
    }
}

impl From<CatalogError> for ApplicationError {
    fn from(value: CatalogError) -> Self {
        Self::Catalog(value)
    }
}

impl From<NativeError> for ApplicationError {
    fn from(value: NativeError) -> Self {
        Self::Native(value)
    }
}

impl From<DispatchError> for ApplicationError {
    fn from(value: DispatchError) -> Self {
        Self::Dispatch(value)
    }
}

impl From<HalError> for ApplicationError {
    fn from(value: HalError) -> Self {
        Self::Hal(value)
    }
}

impl From<ScriptError> for ApplicationError {
    fn from(value: ScriptError) -> Self {
        Self::Script(value)
    }
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(error) => error.fmt(formatter),
            Self::Catalog(error) => error.fmt(formatter),
            Self::Native(error) => error.fmt(formatter),
            Self::Dispatch(error) => error.fmt(formatter),
            Self::Hal(error) => error.fmt(formatter),
            Self::Script(error) => error.fmt(formatter),
        }
    }
}

impl RecoveryClassified for ApplicationError {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::Cli(error) => error.recovery_class(),
            Self::Catalog(error) => error.recovery_class(),
            Self::Native(error) => error.recovery_class(),
            Self::Dispatch(error) => error.recovery_class(),
            Self::Hal(error) => error.recovery_class(),
            Self::Script(error) => error.recovery_class(),
        }
    }
}
