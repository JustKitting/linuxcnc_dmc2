use std::ffi::OsString;

use crate::error::Error;
use crate::integrity::{system_deployments_current, validate_system_deployments};
use crate::layout::Layout;
use crate::platform::{CommandSpec, Platform};
use crate::validation::{require_executable, require_success, run};

pub fn synchronize_system_artifacts(
    platform: &dyn Platform,
    layout: &Layout,
) -> Result<bool, Error> {
    if system_deployments_current(platform, layout)? {
        return Ok(false);
    }

    let installer = layout.project.join("scripts/install_native_module.sh");
    let mut command = CommandSpec::new(require_executable(platform, "sudo")?);
    command.arguments.push(OsString::from("-n"));
    command.arguments.push(OsString::from("--"));
    command.arguments.push(installer.into_os_string());
    command.working_directory = Some(layout.project.clone());

    let output = run(platform, &command)?;
    require_success(&command, &output)?;
    if !output.stderr.is_empty() {
        return Err(Error::ProcessFailed {
            program: command.program,
            status: output.status,
            stdout: output.stdout,
            stderr: output.stderr,
        });
    }
    validate_system_deployments(platform, layout)?;
    Ok(true)
}
