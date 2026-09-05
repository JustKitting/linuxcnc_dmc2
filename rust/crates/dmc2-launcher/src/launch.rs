use std::ffi::OsString;
use std::io::Write;

use crate::cli::Mode;
use crate::deployment;
use crate::error::Error;
use crate::layout::Layout;
use crate::owner;
use crate::platform::{CommandSpec, Platform};
use crate::validation::{self, require_executable};

pub const PERSISTENT_UNIT: &str = "dmc2-linuxcnc";
pub const FAILURE_REPORT_PATH: &str = "/tmp/linuxcnc.report";
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
            deployment::synchronize_system_artifacts(platform, layout)?;
            Action::Replace(direct_command(layout, tools.linuxcnc))
        }
        Mode::Persistent => {
            let tools = validation::validate_live_inputs(platform, layout)?;
            owner::prepare_exclusive_start(platform)?;
            deployment::synchronize_system_artifacts(platform, layout)?;
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
            if completed.status != Some(0) {
                let (failure_report, failure_report_read_error) =
                    match platform.read_file(std::path::Path::new(FAILURE_REPORT_PATH)) {
                        Ok(report) => (report, None),
                        Err(error) => (Vec::new(), Some(error.to_string())),
                    };
                return Err(Error::LinuxCncSessionFailed {
                    program: command.program,
                    status: completed.status,
                    stdout: completed.stdout,
                    stderr: completed.stderr,
                    failure_report_path: FAILURE_REPORT_PATH.into(),
                    failure_report,
                    failure_report_read_error,
                });
            }
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
                "PERSISTENT LIVE SESSION EXITED CLEANLY: {}.service",
                PERSISTENT_UNIT
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
    let mut command = supervised_linuxcnc_command(layout, linuxcnc);
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

fn supervised_linuxcnc_command(layout: &Layout, linuxcnc: std::path::PathBuf) -> CommandSpec {
    let mut command = CommandSpec::new(layout.project.join("native/bin/dmc2-session-supervisor"));
    command.arguments.push(OsString::from("--role"));
    command.arguments.push(OsString::from("linuxcnc-session"));
    command.arguments.push(OsString::from("--journal"));
    command.arguments.push(
        layout
            .project
            .join("var/log/linuxcnc/process-lifecycle.tsv")
            .into_os_string(),
    );
    command.arguments.push(OsString::from("--failure-report"));
    command.arguments.push(OsString::from(FAILURE_REPORT_PATH));
    command.arguments.push(OsString::from("--"));
    command.arguments.push(linuxcnc.into_os_string());
    command.arguments.push(OsString::from("-r"));
    command
        .arguments
        .push(layout.project.join("live/dmc2.ini").into_os_string());
    command.working_directory = Some(layout.project.join("live"));
    command
}

fn persistent_command(
    platform: &dyn Platform,
    layout: &Layout,
    linuxcnc: std::path::PathBuf,
) -> Result<CommandSpec, Error> {
    let systemd_run = require_executable(platform, "systemd-run")?;
    Ok(persistent_command_with_systemd(
        layout,
        linuxcnc,
        systemd_run,
    ))
}

fn persistent_command_with_systemd(
    layout: &Layout,
    linuxcnc: std::path::PathBuf,
    systemd_run: std::path::PathBuf,
) -> CommandSpec {
    let mut command = CommandSpec::new(systemd_run);
    for argument in [
        "--user",
        "--quiet",
        "--unit=dmc2-linuxcnc",
        "--wait",
        "--service-type=exec",
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
    let supervised = supervised_linuxcnc_command(layout, linuxcnc);
    command.arguments.push(supervised.program.into_os_string());
    command.arguments.extend(supervised.arguments);
    command
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn layout() -> Layout {
        Layout {
            project: PathBuf::from("/project"),
            h100: PathBuf::from("/h100"),
        }
    }

    fn supervised_arguments() -> Vec<OsString> {
        [
            "--role",
            "linuxcnc-session",
            "--journal",
            "/project/var/log/linuxcnc/process-lifecycle.tsv",
            "--failure-report",
            "/tmp/linuxcnc.report",
            "--",
            "/usr/bin/linuxcnc",
            "-r",
            "/project/live/dmc2.ini",
        ]
        .into_iter()
        .map(OsString::from)
        .collect()
    }

    #[test]
    fn direct_launch_execs_the_session_owner_with_exact_linuxcnc_arguments() {
        let command = direct_command(&layout(), PathBuf::from("/usr/bin/linuxcnc"));

        assert_eq!(
            command.program,
            PathBuf::from("/project/native/bin/dmc2-session-supervisor")
        );
        assert_eq!(command.arguments, supervised_arguments());
        assert_eq!(
            command.working_directory,
            Some(PathBuf::from("/project/live"))
        );
        assert_eq!(
            command
                .environment
                .get(std::ffi::OsStr::new(REALTIME_ENVIRONMENT_NAME)),
            Some(&OsString::from(REALTIME_ENVIRONMENT_VALUE))
        );
    }

    #[test]
    fn persistent_launch_runs_the_same_session_owner_inside_systemd() {
        let command = persistent_command_with_systemd(
            &layout(),
            PathBuf::from("/usr/bin/linuxcnc"),
            PathBuf::from("/usr/bin/systemd-run"),
        );

        assert_eq!(command.program, PathBuf::from("/usr/bin/systemd-run"));
        let owner_index = command
            .arguments
            .iter()
            .position(|argument| argument == "/project/native/bin/dmc2-session-supervisor")
            .expect("session owner argument");
        assert_eq!(
            &command.arguments[owner_index + 1..],
            supervised_arguments()
        );
        assert!(command
            .arguments
            .contains(&OsString::from("--property=Restart=no")));
        assert!(command.arguments.contains(&OsString::from("--wait")));
        assert!(command
            .arguments
            .contains(&OsString::from("--service-type=exec")));
    }
}
