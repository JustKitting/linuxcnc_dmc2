use std::ffi::OsString;
use std::io::Write;

use crate::cli::Mode;
use crate::deployment;
use crate::error::Error;
use crate::layout::Layout;
use crate::owner;
use crate::platform::{CommandSpec, Platform};
use crate::validation::{self, require_executable, require_success};

pub const PERSISTENT_UNIT: &str = "dmc2-linuxcnc";
pub const REALTIME_ENVIRONMENT_NAME: &str = "LINUXCNC_FORCE_REALTIME";
pub const REALTIME_ENVIRONMENT_VALUE: &str = "1";
pub const PYTHON_BYTECODE_ENVIRONMENT_NAME: &str = "PYTHONDONTWRITEBYTECODE";
pub const PYTHON_BYTECODE_ENVIRONMENT_VALUE: &str = "1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Validate,
    Persistent(CommandSpec),
    Replace(CommandSpec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub action: Action,
}

pub fn prepare(platform: &dyn Platform, layout: &Layout, mode: Mode) -> Result<Plan, Error> {
    let action = match mode {
        Mode::Validate => {
            validation::validate(platform, layout)?;
            Action::Validate
        }
        Mode::Direct => {
            let tools = validation::validate_live_inputs(platform, layout)?;
            owner::prepare_exclusive_start(platform)?;
            deployment::synchronize_realtime_modules(platform, layout)?;
            Action::Replace(direct_command(layout, tools.linuxcnc))
        }
        Mode::Persistent => {
            let tools = validation::validate_live_inputs(platform, layout)?;
            owner::prepare_exclusive_start(platform)?;
            deployment::synchronize_realtime_modules(platform, layout)?;
            Action::Persistent(persistent_command(platform, layout, tools.linuxcnc)?)
        }
    };
    Ok(Plan { action })
}

pub fn execute(platform: &dyn Platform, plan: Plan, output: &mut dyn Write) -> Result<i32, Error> {
    for pass in validation::PASSES {
        writeln!(output, "PASS: {pass}")
            .map_err(|error| Error::os("write launcher output", "/dev/stdout".into(), error))?;
    }
    match plan.action {
        Action::Validate => {
            writeln!(
                output,
                "VALIDATION ONLY — LinuxCNC, Nano serial, and Mesa were not opened."
            )
            .map_err(|error| Error::os("write launcher output", "/dev/stdout".into(), error))?;
            Ok(0)
        }
        Action::Persistent(command) => {
            let completed = validation::run(platform, &command)?;
            require_success(&command, &completed)?;
            if !completed.stderr.is_empty() {
                return Err(Error::ProcessFailed {
                    program: command.program.clone(),
                    status: completed.status,
                    stdout: completed.stdout,
                    stderr: completed.stderr,
                });
            }
            output
                .write_all(&completed.stdout)
                .map_err(|error| Error::os("write service output", "/dev/stdout".into(), error))?;
            if !completed.stdout.is_empty() && !completed.stdout.ends_with(b"\n") {
                output.write_all(b"\n").map_err(|error| {
                    Error::os("write service output", "/dev/stdout".into(), error)
                })?;
            }
            writeln!(
                output,
                concat!(
                    "PERSISTENT LIVE UNIT STARTED: {}.service; inspect with ",
                    "'journalctl --user-unit {} -f'"
                ),
                PERSISTENT_UNIT, PERSISTENT_UNIT
            )
            .map_err(|error| Error::os("write launcher output", "/dev/stdout".into(), error))?;
            Ok(0)
        }
        Action::Replace(command) => match platform.replace_process(&command) {
            Ok(()) => Err(Error::ExecReturned),
            Err(error) => Err(Error::os("replace process", command.program, error)),
        },
    }
}

fn direct_command(layout: &Layout, linuxcnc: std::path::PathBuf) -> CommandSpec {
    let mut command = CommandSpec::new(linuxcnc);
    command.arguments.push(OsString::from("-r"));
    command
        .arguments
        .push(layout.project.join("live/dmc2.ini").into_os_string());
    command.environment.insert(
        OsString::from(REALTIME_ENVIRONMENT_NAME),
        OsString::from(REALTIME_ENVIRONMENT_VALUE),
    );
    command.environment.insert(
        OsString::from(PYTHON_BYTECODE_ENVIRONMENT_NAME),
        OsString::from(PYTHON_BYTECODE_ENVIRONMENT_VALUE),
    );
    command.working_directory = Some(layout.project.join("live"));
    command
}

fn persistent_command(
    platform: &dyn Platform,
    layout: &Layout,
    linuxcnc: std::path::PathBuf,
) -> Result<CommandSpec, Error> {
    let mut command = CommandSpec::new(require_executable(platform, "systemd-run")?);
    for argument in [
        "--user",
        "--quiet",
        "--unit=dmc2-linuxcnc",
        "--setenv=LINUXCNC_FORCE_REALTIME=1",
        "--setenv=PYTHONDONTWRITEBYTECODE=1",
        "--collect",
        "--property=KillMode=control-group",
        "--property=Restart=no",
    ] {
        command.arguments.push(OsString::from(argument));
    }
    let mut working_directory = OsString::from("--working-directory=");
    working_directory.push(layout.project.join("live"));
    command.arguments.push(working_directory);
    command.arguments.push(linuxcnc.into_os_string());
    command.arguments.push(OsString::from("-r"));
    command
        .arguments
        .push(layout.project.join("live/dmc2.ini").into_os_string());
    Ok(command)
}
