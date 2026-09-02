use std::path::{Path, PathBuf};

use crate::embedded;
use crate::error::Error;
use crate::layout::Layout;
use crate::platform::Platform;

pub fn validate_embedded_inputs(platform: &dyn Platform, layout: &Layout) -> Result<(), Error> {
    for file in embedded::FILES {
        let root = match file.root {
            embedded::Root::Project => &layout.project,
            embedded::Root::H100 => &layout.h100,
        };
        let path = root.join(file.relative);
        require_regular_file(platform, &path)?;
        let observed = read(platform, &path)?;
        if observed.as_slice() != file.bytes {
            return Err(Error::EmbeddedFileChanged(path));
        }
    }
    Ok(())
}

pub fn deployments(layout: &Layout) -> Vec<(PathBuf, PathBuf)> {
    let mut deployments = realtime_deployments(layout);
    deployments.extend(userspace_deployments(layout));
    deployments
}

pub fn realtime_deployments(layout: &Layout) -> Vec<(PathBuf, PathBuf)> {
    vec![
        (
            PathBuf::from("/usr/lib/linuxcnc/modules/dmc2_rt.so"),
            layout.project.join("rust/target/release/libdmc2_rt.so"),
        ),
        (
            PathBuf::from("/usr/lib/linuxcnc/modules/h100_spindle.so"),
            layout.h100.join("target/release/h100_spindle.so"),
        ),
    ]
}

pub fn userspace_deployments(layout: &Layout) -> Vec<(PathBuf, PathBuf)> {
    vec![
        (
            layout.project.join("native/bin/dmc2-serial-bridge"),
            layout
                .project
                .join("rust/target/release/dmc2-serial-bridge"),
        ),
        (
            layout.project.join("native/bin/dmc2-task-monitor"),
            layout.project.join("rust/target/release/dmc2-task-monitor"),
        ),
        (
            layout.project.join("native/bin/dmc2-linuxcnc"),
            layout.project.join("rust/target/release/dmc2-linuxcnc"),
        ),
        (
            layout.project.join("native/bin/dmc2ctl"),
            layout.project.join("rust/target/release/dmc2ctl"),
        ),
    ]
}

pub fn validate_deployments(platform: &dyn Platform, layout: &Layout) -> Result<(), Error> {
    validate_deployment_set(platform, deployments(layout))
}

pub fn validate_realtime_deployments(
    platform: &dyn Platform,
    layout: &Layout,
) -> Result<(), Error> {
    validate_deployment_set(platform, realtime_deployments(layout))
}

pub fn validate_userspace_deployments(
    platform: &dyn Platform,
    layout: &Layout,
) -> Result<(), Error> {
    validate_deployment_set(platform, userspace_deployments(layout))
}

pub fn realtime_deployments_current(
    platform: &dyn Platform,
    layout: &Layout,
) -> Result<bool, Error> {
    for (deployed, staged) in realtime_deployments(layout) {
        require_regular_file(platform, &staged)?;
        match platform.is_regular_file(&deployed) {
            Ok(true) => {
                if read(platform, &deployed)? != read(platform, &staged)? {
                    return Ok(false);
                }
            }
            Ok(false) => return Err(Error::NotRegularFile(deployed)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(Error::os("inspect regular file", deployed, error));
            }
        }
    }
    Ok(true)
}

fn validate_deployment_set(
    platform: &dyn Platform,
    deployments: Vec<(PathBuf, PathBuf)>,
) -> Result<(), Error> {
    for (deployed, staged) in deployments {
        require_regular_file(platform, &deployed)?;
        require_regular_file(platform, &staged)?;
        if read(platform, &deployed)? != read(platform, &staged)? {
            return Err(Error::DeploymentMismatch { deployed, staged });
        }
    }
    Ok(())
}

fn require_regular_file(platform: &dyn Platform, path: &Path) -> Result<(), Error> {
    match platform.is_regular_file(path) {
        Ok(true) => Ok(()),
        Ok(false) => Err(Error::NotRegularFile(path.to_path_buf())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(Error::NotRegularFile(path.to_path_buf()))
        }
        Err(error) => Err(Error::os("inspect regular file", path.to_path_buf(), error)),
    }
}

fn read(platform: &dyn Platform, path: &Path) -> Result<Vec<u8>, Error> {
    platform
        .read_file(path)
        .map_err(|error| Error::os("read file", path.to_path_buf(), error))
}
