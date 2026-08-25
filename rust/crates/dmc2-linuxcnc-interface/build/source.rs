use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use super::config::{
    EXPECTED_LINUXCNC_COMMIT, EXPECTED_LINUXCNC_VERSION, INCLUDE_ROOT, SOURCE_ROOT_RELATIVE,
};
use super::process;

pub(crate) struct Headers {
    pub(crate) emc: String,
    pub(crate) motion: String,
    pub(crate) emcmotcfg: String,
    pub(crate) interp_return: String,
    pub(crate) nml: String,
    pub(crate) rcs: String,
    pub(crate) stat_msg: String,
    pub(crate) canon: String,
    pub(crate) kinematics: String,
    pub(crate) motion_types: String,
    pub(crate) debug_flags: String,
    pub(crate) state_tag: String,
    pub(crate) cmd_msg: String,
}

pub(crate) fn verify_checkout() -> PathBuf {
    let source_root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"))
            .join(SOURCE_ROOT_RELATIVE);
    let source_root_text = source_root
        .to_str()
        .expect("LinuxCNC source path is not valid UTF-8");
    let installed_version = process::text("linuxcnc_var", &["LINUXCNCVERSION"]);
    assert_eq!(
        installed_version.trim(),
        EXPECTED_LINUXCNC_VERSION,
        "refusing to generate controller bindings for another LinuxCNC version"
    );
    let source_commit = process::text("git", &["-C", source_root_text, "rev-parse", "HEAD"]);
    assert_eq!(
        source_commit.trim(),
        EXPECTED_LINUXCNC_COMMIT,
        "the controller bindings require the official LinuxCNC 2.9.10 source commit"
    );
    let source_changes = process::text("git", &["-C", source_root_text, "status", "--porcelain"]);
    assert!(
        source_changes.trim().is_empty(),
        "the LinuxCNC source used for controller bindings has local modifications"
    );
    source_root
}

fn relative_header_path(name: &str) -> &'static str {
    match name {
        "emc.hh" => "src/emc/nml_intf/emc.hh",
        "emc_nml.hh" => "src/emc/nml_intf/emc_nml.hh",
        "motion.h" => "src/emc/motion/motion.h",
        "emcmotcfg.h" => "src/emc/motion/emcmotcfg.h",
        "interp_return.hh" => "src/emc/nml_intf/interp_return.hh",
        "nml.hh" => "src/libnml/nml/nml.hh",
        "rcs.hh" => "src/libnml/rcs/rcs.hh",
        "stat_msg.hh" => "src/libnml/nml/stat_msg.hh",
        "canon.hh" => "src/emc/nml_intf/canon.hh",
        "kinematics.h" => "src/emc/kinematics/kinematics.h",
        "motion_types.h" => "src/emc/nml_intf/motion_types.h",
        "debugflags.h" => "src/emc/nml_intf/debugflags.h",
        "state_tag.h" => "src/emc/motion/state_tag.h",
        "cmd_msg.hh" => "src/libnml/nml/cmd_msg.hh",
        _ => panic!("no controller source mapping for LinuxCNC header {name}"),
    }
}

fn read_header(source_root: &Path, name: &str) -> String {
    let installed_path = Path::new(INCLUDE_ROOT).join(name);
    let source_path = source_root.join(relative_header_path(name));
    println!("cargo:rerun-if-changed={}", installed_path.display());
    println!("cargo:rerun-if-changed={}", source_path.display());
    let installed = fs::read_to_string(&installed_path).unwrap_or_else(|error| {
        panic!(
            "failed to read installed header {}: {error}",
            installed_path.display()
        )
    });
    let source = fs::read_to_string(&source_path).unwrap_or_else(|error| {
        panic!(
            "failed to read official source header {}: {error}",
            source_path.display()
        )
    });
    assert_eq!(
        source, installed,
        "installed {name} differs from the LinuxCNC source used by the controller"
    );
    source
}

pub(crate) fn load_headers(source_root: &Path) -> Headers {
    // The concrete status classes whose sizes are validated by the program
    // are defined here rather than in emc.hh.
    drop(read_header(source_root, "emc_nml.hh"));
    Headers {
        emc: read_header(source_root, "emc.hh"),
        motion: read_header(source_root, "motion.h"),
        emcmotcfg: read_header(source_root, "emcmotcfg.h"),
        interp_return: read_header(source_root, "interp_return.hh"),
        nml: read_header(source_root, "nml.hh"),
        rcs: read_header(source_root, "rcs.hh"),
        stat_msg: read_header(source_root, "stat_msg.hh"),
        canon: read_header(source_root, "canon.hh"),
        kinematics: read_header(source_root, "kinematics.h"),
        motion_types: read_header(source_root, "motion_types.h"),
        debug_flags: read_header(source_root, "debugflags.h"),
        state_tag: read_header(source_root, "state_tag.h"),
        cmd_msg: read_header(source_root, "cmd_msg.hh"),
    }
}
