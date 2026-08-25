use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use super::config::{
    EXPECTED_LINUXCNC_COMMIT, EXPECTED_LINUXCNC_VERSION, EXPECTED_PUBLIC_ENUM_HEADER_COUNT,
    EXPECTED_PUBLIC_HEADER_COUNT, INCLUDE_ROOT, SOURCE_ROOT_RELATIVE,
};
use super::process;

#[derive(Clone)]
pub(crate) struct PublicHeader {
    pub(crate) name: String,
    pub(crate) installed_path: PathBuf,
    pub(crate) source_path: PathBuf,
    pub(crate) source: String,
}

pub(crate) struct PublicEnumHeader {
    pub(crate) name: String,
    pub(crate) source: String,
}

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
    pub(crate) fn named(&self) -> [(&'static str, &str); 17] {
        [
            ("emc.hh", &self.emc),
            ("emc_nml.hh", &self.emc_nml),
            ("motion.h", &self.motion),
            ("emcmotcfg.h", &self.emcmotcfg),
            ("interp_return.hh", &self.interp_return),
            ("nml.hh", &self.nml),
            ("nml_oi.hh", &self.nml_oi),
            ("rcs.hh", &self.rcs),
            ("stat_msg.hh", &self.stat_msg),
            ("canon.hh", &self.canon),
            ("kinematics.h", &self.kinematics),
            ("motion_types.h", &self.motion_types),
            ("debugflags.h", &self.debug_flags),
            ("state_tag.h", &self.state_tag),
            ("usrmotintf.h", &self.usrmotintf),
            ("cms.hh", &self.cms),
            ("cmd_msg.hh", &self.cmd_msg),
        ]
    }

    pub(crate) fn all(&self) -> [&str; 17] {
        self.named().map(|(_, source)| source)
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

fn contains_enum_token(source: &str) -> bool {
    source.match_indices("enum").any(|(index, _)| {
        let before = source[..index].bytes().next_back();
        let after = source[index + 4..].bytes().next();
        let is_identifier = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
        before.is_none_or(|byte| !is_identifier(byte))
            && after.is_none_or(|byte| !is_identifier(byte))
    })
}

fn find_source_headers(directory: &Path, name: &str, matches: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
    {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "failed to inspect an entry in {}: {error}",
                directory.display()
            )
        });
        let path = entry.path();
        if path.file_name().and_then(|value| value.to_str()) == Some(".git") {
            continue;
        }
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("failed to inspect {}: {error}", path.display()));
        if file_type.is_dir() {
            find_source_headers(&path, name, matches);
        } else if file_type.is_file()
            && path.file_name().and_then(|value| value.to_str()) == Some(name)
        {
            matches.push(path);
        }
    }
}

fn source_header_path(source_root: &Path, name: &str) -> PathBuf {
    let mut matches = Vec::new();
    find_source_headers(&source_root.join("src"), name, &mut matches);
    if name == "hal.h" {
        matches.retain(|path| path.ends_with("src/hal/hal.h"));
    }
    assert_eq!(
        matches.len(),
        1,
        "expected one authoritative LinuxCNC source header named {name}, found {matches:?}"
    );
    matches.remove(0)
}

fn target_lines(preprocessed: &str, target: &str) -> String {
    let mut active = false;
    let mut selected = String::new();
    for line in preprocessed.lines() {
        if line.starts_with("# ") {
            let Some(open_quote) = line.find('"') else {
                active = false;
                continue;
            };
            let remainder = &line[open_quote + 1..];
            let Some(close_quote) = remainder.find('"') else {
                active = false;
                continue;
            };
            active = &remainder[..close_quote] == target;
        } else if active {
            selected.push_str(line);
            selected.push('\n');
        }
    }
    selected
}

fn preprocess_public_header(installed_path: &Path, name: &str) -> String {
    let installed = installed_path
        .to_str()
        .expect("installed LinuxCNC header path is not UTF-8");
    if name == "interp_internal.hh" {
        return process::text(
            "g++",
            &[
                "-std=c++17",
                "-E",
                "-P",
                "-fpreprocessed",
                "-x",
                "c++",
                installed,
            ],
        );
    }
    let preprocessed = process::text(
        "g++",
        &[
            "-std=c++17",
            "-E",
            "-x",
            "c++",
            "-DULAPI",
            "-I",
            INCLUDE_ROOT,
            installed,
        ],
    );
    target_lines(&preprocessed, installed)
}

pub(crate) fn load_public_headers(source_root: &Path) -> Vec<PublicHeader> {
    let mut installed_paths = fs::read_dir(INCLUDE_ROOT)
        .unwrap_or_else(|error| panic!("failed to read {INCLUDE_ROOT}: {error}"))
        .filter_map(|entry| {
            let entry = entry.unwrap_or_else(|error| {
                panic!("failed to inspect an entry in {INCLUDE_ROOT}: {error}")
            });
            let path = entry.path();
            let file_type = entry
                .file_type()
                .unwrap_or_else(|error| panic!("failed to inspect {}: {error}", path.display()));
            if !file_type.is_file() {
                return None;
            }
            Some(path)
        })
        .collect::<Vec<_>>();
    installed_paths.sort();
    assert_eq!(
        installed_paths.len(),
        EXPECTED_PUBLIC_HEADER_COUNT,
        "the installed LinuxCNC public-header inventory changed"
    );

    installed_paths
        .into_iter()
        .map(|installed_path| {
            let name = installed_path
                .file_name()
                .and_then(|value| value.to_str())
                .expect("installed LinuxCNC header name is not UTF-8")
                .to_owned();
            let source_path = source_header_path(source_root, &name);
            println!("cargo:rerun-if-changed={}", installed_path.display());
            println!("cargo:rerun-if-changed={}", source_path.display());
            let installed_bytes = fs::read(&installed_path).unwrap_or_else(|error| {
                panic!("failed to read {}: {error}", installed_path.display())
            });
            let source_bytes = fs::read(&source_path).unwrap_or_else(|error| {
                panic!("failed to read {}: {error}", source_path.display())
            });
            assert_eq!(
                source_bytes, installed_bytes,
                "installed {name} does not exactly match pulled LinuxCNC v2.9.10 source"
            );
            let source = String::from_utf8(installed_bytes)
                .unwrap_or_else(|error| panic!("installed {name} is not UTF-8: {error}"));
            PublicHeader {
                name,
                installed_path,
                source_path,
                source,
            }
        })
        .collect()
}

pub(crate) fn load_public_enum_headers(headers: &[PublicHeader]) -> Vec<PublicEnumHeader> {
    let enum_headers = headers
        .iter()
        .filter(|header| contains_enum_token(&header.source))
        .map(|header| PublicEnumHeader {
            name: header.name.clone(),
            source: preprocess_public_header(&header.installed_path, &header.name),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        enum_headers.len(),
        EXPECTED_PUBLIC_ENUM_HEADER_COUNT,
        "the installed LinuxCNC public-header enum inventory changed"
    );
    enum_headers
}
