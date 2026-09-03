use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::{Error, OwnerMatch};
use crate::platform::{CommandSpec, Platform};
use crate::validation::{require_executable, run};

pub const CONFLICT_PATTERNS: &[&str] = &[
    "[/]usr/bin/[l]inuxcnc([[:space:]]|$)",
    "[l]inuxcncsvr",
    "[m]illtask",
    "[d]mc2-process-supervisor",
    "[d]mc2-session-supervisor",
    "[d]mc2-serial-bridge",
    "[d]mc2-task-monitor",
    "[p]endant_cnc/control.py.*--live",
    "[h]alrun",
    "[r]tapi_app",
];
pub const LINUXCNC_LOCK_PATH: &str = "/tmp/linuxcnc.lock";

pub fn prepare_exclusive_start(platform: &dyn Platform) -> Result<(), Error> {
    assert_exclusive(platform)?;
    let lock = Path::new(LINUXCNC_LOCK_PATH);
    platform.remove_file_if_exists(lock).map_err(|error| {
        Error::os(
            "remove proven-stale LinuxCNC lock",
            PathBuf::from(lock),
            error,
        )
    })?;
    Ok(())
}

pub fn assert_exclusive(platform: &dyn Platform) -> Result<(), Error> {
    let pgrep = require_executable(platform, "pgrep")?;
    let mut conflicts = Vec::new();
    for pattern in CONFLICT_PATTERNS {
        let mut command = CommandSpec::new(&pgrep);
        command.arguments.push(OsString::from("-f"));
        command.arguments.push(OsString::from(pattern));
        let output = run(platform, &command)?;
        match output.status {
            Some(0) => conflicts.push(OwnerMatch {
                pattern,
                stdout: output.stdout,
                stderr: output.stderr,
            }),
            Some(1) if output.stdout.is_empty() && output.stderr.is_empty() => {}
            status => {
                return Err(Error::OwnerProbe {
                    pattern,
                    status,
                    stdout: output.stdout,
                    stderr: output.stderr,
                });
            }
        }
    }
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(Error::OwnerConflict(conflicts))
    }
}
