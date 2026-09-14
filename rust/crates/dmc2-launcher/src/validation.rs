use std::ffi::OsString;
use std::path::PathBuf;

use crate::error::Error;
use crate::hal_validation;
use crate::integrity::{
    validate_deployments, validate_embedded_inputs, validate_userspace_deployments,
};
use crate::layout::Layout;
use crate::platform::{CommandSpec, Platform, ProcessOutput};

pub const EXPECTED_LINUXCNC_VERSION: &[u8] = b"2.9.10\n";

pub const PASSES: &[&str] = &[
    "executable integration inputs match this build; runtime data is validated by its consuming operation",
    "all realtime modules, system libraries, and userspace binaries byte-match release artifacts",
    "LinuxCNC 2.9.10 is installed",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedTools {
    pub linuxcnc: std::path::PathBuf,
}

pub fn validate(platform: &dyn Platform, layout: &Layout) -> Result<ValidatedTools, Error> {
    validate_embedded_inputs(platform, layout)?;
    hal_validation::validate(platform, layout)?;
    validate_deployments(platform, layout)?;
    validate_tools(platform)
}

pub fn validate_live_inputs(
    platform: &dyn Platform,
    layout: &Layout,
) -> Result<ValidatedTools, Error> {
    validate_embedded_inputs(platform, layout)?;
    hal_validation::validate(platform, layout)?;
    validate_userspace_deployments(platform, layout)?;
    validate_tools(platform)
}

fn validate_tools(platform: &dyn Platform) -> Result<ValidatedTools, Error> {
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
