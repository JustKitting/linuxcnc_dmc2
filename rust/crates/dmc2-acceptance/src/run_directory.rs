use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::failure::{Failure, FailureCode, Result};

pub(crate) struct RunDirectory {
    path: PathBuf,
    preserve: bool,
}

impl RunDirectory {
    pub(crate) fn create(project_root: &Path) -> Result<Self> {
        let epoch_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| {
                Failure::new(
                    FailureCode::FileSystem,
                    format!("system clock precedes UNIX epoch: {error}"),
                )
            })?
            .as_millis();
        let parent = project_root.join("var/log/tests/linuxcnc-motion");
        fs::create_dir_all(&parent).map_err(|error| {
            Failure::io(
                FailureCode::FileSystem,
                "create organized acceptance log directory",
                error,
            )
        })?;
        let path = parent.join(format!("run-{}-{epoch_millis}", std::process::id()));
        fs::create_dir(&path).map_err(|error| {
            Failure::io(
                FailureCode::FileSystem,
                "create unique acceptance run directory",
                error,
            )
        })?;
        Ok(Self {
            path,
            preserve: true,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn render_ini(
        &self,
        project_root: &Path,
        pendant_port: &str,
        rsh_port: u16,
    ) -> Result<PathBuf> {
        let template_path = project_root.join("tests/linuxcnc-motion/motion.ini.in");
        let template = fs::read_to_string(&template_path).map_err(|error| {
            Failure::io(
                FailureCode::FileSystem,
                "read motion acceptance INI template",
                error,
            )
        })?;
        let rendered = template
            .replace("@PROJECT_ROOT@", &project_root.display().to_string())
            .replace("@RUN_DIRECTORY@", &self.path.display().to_string())
            .replace("@PENDANT_PORT@", pendant_port)
            .replace("@RSH_PORT@", &rsh_port.to_string());
        if rendered.contains('@') {
            return Err(Failure::new(
                FailureCode::Artifact,
                format!(
                    "unresolved template marker remains in {}",
                    template_path.display()
                ),
            ));
        }
        let ini_path = self.path.join("motion.ini");
        fs::write(&ini_path, rendered).map_err(|error| {
            Failure::io(
                FailureCode::FileSystem,
                "write rendered motion acceptance INI",
                error,
            )
        })?;
        Ok(ini_path)
    }

    pub(crate) fn mark_success(&mut self) {
        self.preserve = false;
    }
}

impl Drop for RunDirectory {
    fn drop(&mut self) {
        if self.preserve {
            eprintln!(
                "acceptance failure artifacts preserved at {}",
                self.path.display()
            );
            return;
        }
        if self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("run-"))
        {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
