use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::failure::{Failure, FailureCode, Result};

const REQUIRED_LINUXCNC_VERSION: &str = "2.9.10";

pub(crate) fn default_project_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .map_err(|error| Failure::io(FailureCode::FileSystem, "canonicalize project root", error))
}

pub(crate) fn require_linuxcnc_2_9_10() -> Result<()> {
    let output = Command::new("linuxcnc_var")
        .arg("LINUXCNCVERSION")
        .output()
        .map_err(|error| {
            Failure::io(
                FailureCode::LinuxCncVersion,
                "execute linuxcnc_var LINUXCNCVERSION",
                error,
            )
        })?;
    if !output.status.success() {
        return Err(Failure::new(
            FailureCode::LinuxCncVersion,
            format!(
                "linuxcnc_var exited {}; stderr={:?}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }
    let observed = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if observed != REQUIRED_LINUXCNC_VERSION {
        return Err(Failure::new(
            FailureCode::LinuxCncVersion,
            format!("required={REQUIRED_LINUXCNC_VERSION}; observed={observed:?}"),
        ));
    }
    Ok(())
}

pub(crate) fn require_no_active_realtime_host() -> Result<()> {
    let entries = fs::read_dir("/proc").map_err(|error| {
        Failure::io(
            FailureCode::RealtimeBusy,
            "enumerate /proc for rtapi_app",
            error,
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            Failure::io(FailureCode::RealtimeBusy, "read /proc process entry", error)
        })?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let comm_path = entry.path().join("comm");
        let Ok(comm) = fs::read_to_string(comm_path) else {
            continue;
        };
        if comm.trim() == "rtapi_app" {
            return Err(Failure::new(
                FailureCode::RealtimeBusy,
                format!(
                    "rtapi_app pid={pid} is active; the isolated motmod test did not start and no hardware state was changed"
                ),
            ));
        }
    }
    Ok(())
}

pub(crate) fn deployment_identity_issues(project_root: &Path) -> Vec<String> {
    let comparisons = [
        (
            project_root.join("rust/target/release/libdmc2_rt.so"),
            PathBuf::from("/usr/lib/linuxcnc/modules/dmc2_rt.so"),
            "realtime module",
        ),
        (
            project_root.join("rust/target/release/dmc2-serial-bridge"),
            project_root.join("native/bin/dmc2-serial-bridge"),
            "serial bridge",
        ),
        (
            project_root.join("rust/target/release/dmc2-task-monitor"),
            project_root.join("native/bin/dmc2-task-monitor"),
            "task monitor",
        ),
    ];

    comparisons
        .into_iter()
        .filter_map(|(built, deployed, label)| compare_pair(&built, &deployed, label).err())
        .collect()
}

fn compare_pair(built: &Path, deployed: &Path, label: &str) -> std::result::Result<(), String> {
    let built_bytes = fs::read(built).map_err(|error| {
        format!(
            "{label}: cannot read built artifact {}: {error}",
            built.display()
        )
    })?;
    let deployed_bytes = fs::read(deployed).map_err(|error| {
        format!(
            "{label}: cannot read deployed artifact {}: {error}",
            deployed.display()
        )
    })?;
    if built_bytes == deployed_bytes {
        Ok(())
    } else {
        Err(format!(
            "{label}: built artifact {} is not byte-identical to deployed artifact {}",
            built.display(),
            deployed.display()
        ))
    }
}

pub(crate) fn require_test_assets(project_root: &Path) -> Result<()> {
    for relative in [
        "tests/linuxcnc-motion/motion.ini.in",
        "tests/linuxcnc-motion/machine.hal",
        "live/hal/pendant_input_sources.hal",
        "live/hal/pendant_motion_contract.hal",
        "native/bin/dmc2-serial-bridge",
        "native/bin/dmc2-task-monitor",
        "python/dmc2_axis/diagnostic_validator.py",
        "python/dmc2_axis/journal_validator.py",
    ] {
        let path = project_root.join(relative);
        if !path.is_file() {
            return Err(Failure::new(
                FailureCode::Artifact,
                format!("required regular file is missing: {}", path.display()),
            ));
        }
    }
    Ok(())
}
