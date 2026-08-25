use std::ffi::OsString;
use std::path::PathBuf;

use crate::error::Error;
use crate::integrity::{validate_deployments, validate_embedded_inputs};
use crate::layout::Layout;
use crate::platform::{CommandSpec, Platform, ProcessOutput};

pub const EXPECTED_LINUXCNC_VERSION: &[u8] = b"2.9.10\n";
pub const EXPECTED_TASK_MONITOR_VALIDATION: &[u8] = b"dmc2-task-monitor: program validation passed; snapshot_abi=0x00020911 snapshot_size=11672 snapshot_fields=1109 native_copy_fields=1100 rust_derived_fields=9 copy_rounds=21 all_bytes_accounted=1\n";

pub const PASSES: &[&str] = &[
    "all live launch inputs byte-match the offline-tested build",
    "all realtime modules and userspace binaries byte-match verified releases",
    "LinuxCNC 2.9.10 is installed",
    "the task monitor validates every byte copied through its native status boundary",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedTools {
    pub linuxcnc: std::path::PathBuf,
}

pub fn validate(platform: &dyn Platform, layout: &Layout) -> Result<ValidatedTools, Error> {
    validate_embedded_inputs(platform, layout)?;
    validate_deployments(platform, layout)?;

    let version_program = require_executable(platform, "linuxcnc_var")?;
    let mut version = CommandSpec::new(version_program);
    version.arguments.push(OsString::from("LINUXCNCVERSION"));
    let output = run(platform, &version)?;
    require_success(&version, &output)?;
    if output.stdout != EXPECTED_LINUXCNC_VERSION || !output.stderr.is_empty() {
        return Err(Error::LinuxCncVersion {
            stdout: output.stdout,
            stderr: output.stderr,
        });
    }

    let linuxcnc = require_executable(platform, "linuxcnc")?;

    let monitor = layout.project.join("native/bin/dmc2-task-monitor");
    let mut validation = CommandSpec::new(monitor);
    validation.arguments.push(OsString::from("--validate"));
    let output = run(platform, &validation)?;
    require_success(&validation, &output)?;
    if output.stdout != EXPECTED_TASK_MONITOR_VALIDATION || !output.stderr.is_empty() {
        return Err(Error::ProgramValidation {
            stdout: output.stdout,
            stderr: output.stderr,
        });
    }
    Ok(ValidatedTools { linuxcnc })
}

pub fn require_executable(
    platform: &dyn Platform,
    name: &'static str,
) -> Result<std::path::PathBuf, Error> {
    platform
        .find_executable(name)
        .map_err(|error| Error::os("locate executable", PathBuf::from(name), error))?
        .ok_or(Error::ExecutableUnavailable(name))
}

pub fn run(platform: &dyn Platform, command: &CommandSpec) -> Result<ProcessOutput, Error> {
    platform
        .run(command)
        .map_err(|error| Error::os("execute process", command.program.clone(), error))
}

pub fn require_success(command: &CommandSpec, output: &ProcessOutput) -> Result<(), Error> {
    if output.status == Some(0) {
        return Ok(());
    }
    Err(Error::ProcessFailed {
        program: command.program.clone(),
        status: output.status,
        stdout: output.stdout.clone(),
        stderr: output.stderr.clone(),
    })
}
