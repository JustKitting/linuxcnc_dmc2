use std::ffi::OsString;

use crate::error::{Error, OwnerMatch};
use crate::platform::{CommandSpec, Platform};
use crate::validation::{require_executable, run};

pub const CONFLICT_PATTERNS: &[&str] = &[
    "[l]inuxcnc.*dmc2.ini",
    "[p]endant_cnc/control.py.*--live",
    "[h]alrun",
    "[r]tapi_app",
];

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
