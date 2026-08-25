use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use super::config::{
    EXPECTED_LINUXCNC_COMMIT, EXPECTED_LINUXCNC_VERSION, INCLUDE_ROOT, SOURCE_ROOT_RELATIVE,
};
use super::process;

pub(crate) struct Headers {
    pub(crate) emc: String,
    pub(crate) emc_nml: String,
    pub(crate) motion: String,
    pub(crate) emcmotcfg: String,
    pub(crate) interp_return: String,
    pub(crate) nml: String,
    pub(crate) nml_oi: String,
    pub(crate) rcs: String,
    pub(crate) stat_msg: String,
    pub(crate) canon: String,
    pub(crate) kinematics: String,
    pub(crate) motion_types: String,
    pub(crate) debug_flags: String,
    pub(crate) state_tag: String,
    pub(crate) usrmotintf: String,
    pub(crate) cms: String,
    pub(crate) cmd_msg: String,
}

impl Headers {
    pub(crate) fn all(&self) -> [&str; 17] {
        [
            &self.emc,
            &self.emc_nml,
            &self.motion,
            &self.emcmotcfg,
            &self.interp_return,
            &self.nml,
            &self.nml_oi,
            &self.rcs,
            &self.stat_msg,
            &self.canon,
            &self.kinematics,
            &self.motion_types,
            &self.debug_flags,
            &self.state_tag,
            &self.usrmotintf,
            &self.cms,
            &self.cmd_msg,
        ]
    }
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
        "refusing to generate an interface catalog for an unaudited LinuxCNC version"
    );
    let source_commit = process::text("git", &["-C", source_root_text, "rev-parse", "HEAD"]);
    assert_eq!(
        source_commit.trim(),
        EXPECTED_LINUXCNC_COMMIT,
        "pulled LinuxCNC source is not the audited v2.9.10 commit"
    );
    let source_changes = process::text("git", &["-C", source_root_text, "status", "--porcelain"]);
    assert!(
        source_changes.trim().is_empty(),
        "pulled LinuxCNC v2.9.10 source has local modifications"
    );
    source_root
}

fn relative_header_path(name: &str) -> String {
    match name {
        "emc.hh" | "interp_return.hh" | "canon.hh" | "motion_types.h" | "debugflags.h" => {
            format!("src/emc/nml_intf/{name}")
        }
        "emc_nml.hh" => "src/emc/nml_intf/emc_nml.hh".to_owned(),
        "motion.h" => "src/emc/motion/motion.h".to_owned(),
        "emcmotcfg.h" => "src/emc/motion/emcmotcfg.h".to_owned(),
        "kinematics.h" => "src/emc/kinematics/kinematics.h".to_owned(),
        "nml.hh" | "nml_oi.hh" => format!("src/libnml/nml/{name}"),
        "rcs.hh" => "src/libnml/rcs/rcs.hh".to_owned(),
        "stat_msg.hh" => "src/libnml/nml/stat_msg.hh".to_owned(),
        "cmd_msg.hh" => "src/libnml/nml/cmd_msg.hh".to_owned(),
        "cms.hh" => "src/libnml/cms/cms.hh".to_owned(),
        "state_tag.h" => "src/emc/motion/state_tag.h".to_owned(),
        "usrmotintf.h" => "src/emc/motion/usrmotintf.h".to_owned(),
        _ => panic!("no audited LinuxCNC source mapping for {name}"),
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
            "failed to read pulled source {}: {error}",
            source_path.display()
        )
    });
    assert_eq!(
        source, installed,
        "installed {name} does not exactly match pulled LinuxCNC v2.9.10 source"
    );
    source
}

pub(crate) fn load_headers(source_root: &Path) -> Headers {
    Headers {
        emc: read_header(source_root, "emc.hh"),
        emc_nml: read_header(source_root, "emc_nml.hh"),
        motion: read_header(source_root, "motion.h"),
        emcmotcfg: read_header(source_root, "emcmotcfg.h"),
        interp_return: read_header(source_root, "interp_return.hh"),
        nml: read_header(source_root, "nml.hh"),
        nml_oi: read_header(source_root, "nml_oi.hh"),
        rcs: read_header(source_root, "rcs.hh"),
        stat_msg: read_header(source_root, "stat_msg.hh"),
        canon: read_header(source_root, "canon.hh"),
        kinematics: read_header(source_root, "kinematics.h"),
        motion_types: read_header(source_root, "motion_types.h"),
        debug_flags: read_header(source_root, "debugflags.h"),
        state_tag: read_header(source_root, "state_tag.h"),
        usrmotintf: read_header(source_root, "usrmotintf.h"),
        cms: read_header(source_root, "cms.hh"),
        cmd_msg: read_header(source_root, "cmd_msg.hh"),
    }
}

pub(crate) fn read_interpreter_errors(source_root: &Path) -> String {
    let path = source_root.join("src/emc/rs274ngc/rs274ngc_return.hh");
    println!("cargo:rerun-if-changed={}", path.display());
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}
