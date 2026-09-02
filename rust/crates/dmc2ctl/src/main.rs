mod catalog;
mod cli;
mod dispatch;
mod hal;
mod native;

use std::fmt;

use catalog::{Catalog, CatalogError, Operation, OperationKind};
use cli::{Action, CliError};
use dispatch::DispatchError;
use hal::HalError;
use native::{ControlBackend, NativeError, Receipt, Session, Status};

fn main() {
    match run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("dmc2ctl: {error}");
            eprintln!("{}", cli::USAGE);
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), ApplicationError> {
    let Some(arguments) = cli::parse_environment()? else {
        println!("{}", cli::USAGE);
        return Ok(());
    };
    let catalog = Catalog::open(arguments.catalog)?;
    match arguments.action {
        Action::List => print_operations(&catalog),
        Action::Describe(id) => print_operation(catalog.operation(&id)?),
        Action::Status => {
            let mut session = Session::open(&arguments.nml_file)?;
            print_status(&session.status()?);
        }
        Action::Execute(id) => {
            let operation = catalog.operation(&id)?;
            let physical_estop_pressed = if operation
                .prerequisites
                .iter()
                .any(|value| value == "physical-estop-released")
            {
                Some(hal::read_bit(hal::PHYSICAL_PENDANT_ESTOP_PIN)?)
            } else {
                None
            };
            let mut session = Session::open(&arguments.nml_file)?;
            let receipt =
                dispatch::execute_control(operation, &mut session, physical_estop_pressed)?;
            print_receipt(operation, &receipt);
        }
        Action::Load(id) => {
            let operation = catalog.operation(&id)?;
            let mut session = Session::open(&arguments.nml_file)?;
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
            let operation = catalog.operation(&id)?;
            let mut session = Session::open(&arguments.nml_file)?;
            let receipt = dispatch::run_program(&catalog, operation, &mut session)?;
            println!(
                "operation={} action=run command_serial={} echo_serial={} rcs_status={}",
                operation.id,
                receipt.command_serial_number,
                receipt.echo_serial_number,
                receipt.rcs_status
            );
        }
    }
    Ok(())
}

fn print_operations(catalog: &Catalog) {
    println!("catalog={}", catalog.path().display());
    println!("id\tkind\tlabel\tdriver\teffects\tprerequisites");
    for operation in catalog
        .operations()
        .filter(|operation| operation.kind != OperationKind::Internal)
    {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            operation.id,
            operation.kind.name(),
            operation.label,
            operation.driver,
            operation.effects.join(";"),
            operation.prerequisites.join(";")
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
    println!("effects={}", operation.effects.join(";"));
    println!("prerequisites={}", operation.prerequisites.join(";"));
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

fn print_receipt(operation: &Operation, receipt: &Receipt) {
    println!(
        "operation={} action=execute command_serial={} echo_serial={} rcs_status={}",
        operation.id, receipt.command_serial_number, receipt.echo_serial_number, receipt.rcs_status
    );
}

#[derive(Debug)]
enum ApplicationError {
    Cli(CliError),
    Catalog(CatalogError),
    Native(NativeError),
    Dispatch(DispatchError),
    Hal(HalError),
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

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(error) => error.fmt(formatter),
            Self::Catalog(error) => error.fmt(formatter),
            Self::Native(error) => error.fmt(formatter),
            Self::Dispatch(error) => error.fmt(formatter),
            Self::Hal(error) => error.fmt(formatter),
        }
    }
}
