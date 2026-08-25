use std::ffi::OsString;
use std::path::PathBuf;

use crate::error::Error;
use crate::integrity::{validate_deployments, validate_embedded_inputs};
use crate::layout::Layout;
use crate::platform::{CommandSpec, Platform, ProcessOutput};

pub const EXPECTED_LINUXCNC_VERSION: &[u8] = b"2.9.10\n";
pub const EXPECTED_INTERFACE_AUDIT: &[u8] = b"dmc2-task-monitor: offline validation passed; LinuxCNC=2.9.10 source=86cdca76fa2a36274c432caa21952b23c267989a catalog_domains=91 catalog_codes=920 enum_headers=30 enum_declarations=79 interpreter_errors=198 status_messages=12 error_messages=6 snapshot_abi=0x00020911 snapshot_size=11672 snapshot_fields=1109 copy_rounds=21 all_bytes_accounted=1\n";

pub const PASSES: &[&str] = &[
    "all live launch inputs byte-match the offline-tested build",
    "all realtime modules and userspace binaries byte-match verified releases",
    "LinuxCNC 2.9.10 is installed",
    "compiled LinuxCNC interface audit accounts for every generated code and snapshot byte",
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
    let mut audit = CommandSpec::new(monitor);
    audit.arguments.push(OsString::from("--validate"));
    let output = run(platform, &audit)?;
    require_success(&audit, &output)?;
    if output.stdout != EXPECTED_INTERFACE_AUDIT || !output.stderr.is_empty() {
        return Err(Error::InterfaceAudit {
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
