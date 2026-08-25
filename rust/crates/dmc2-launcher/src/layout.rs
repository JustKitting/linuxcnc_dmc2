use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::platform::Platform;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub project: PathBuf,
    pub h100: PathBuf,
}

const PROJECT_MARKERS: &[&str] = &["live/dmc2.ini", "live_requirements.json", "rust/Cargo.toml"];

pub fn discover(platform: &dyn Platform) -> Result<Layout, Error> {
    let executable = platform.current_executable().map_err(|error| {
        Error::os(
            "resolve current executable",
            PathBuf::from("/proc/self/exe"),
            error,
        )
    })?;
    discover_from(platform, &executable)
}

pub fn discover_from(platform: &dyn Platform, start: &Path) -> Result<Layout, Error> {
    for ancestor in start.ancestors() {
        let mut matches = true;
        for marker in PROJECT_MARKERS {
            match platform.is_regular_file(&ancestor.join(marker)) {
                Ok(true) => {}
                Ok(false) => {
                    matches = false;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    matches = false;
                    break;
                }
                Err(error) => {
                    return Err(Error::os(
                        "inspect project marker",
                        ancestor.join(marker),
                        error,
                    ));
                }
            }
        }
        if matches {
            let project = ancestor.to_path_buf();
            let Some(parent) = project.parent() else {
                return Err(Error::ProjectRootNotFound(start.to_path_buf()));
            };
            return Ok(Layout {
                h100: parent.join("h100_modbus"),
                project,
            });
        }
    }
    Err(Error::ProjectRootNotFound(start.to_path_buf()))
}
