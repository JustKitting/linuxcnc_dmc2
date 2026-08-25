use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use super::config::{
    EXPECTED_LINUXCNC_COMMIT, EXPECTED_LINUXCNC_VERSION, HAL_HEADER, SOURCE_HAL_HEADER_RELATIVE,
    SOURCE_ROOT_RELATIVE,
};
use super::process;

pub(crate) fn verify_audited_source() {
    let installed_header = Path::new(HAL_HEADER);
    assert!(
        installed_header.is_file(),
        "the installed LinuxCNC HAL header is missing: {HAL_HEADER}"
    );

    let source_root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"))
            .join(SOURCE_ROOT_RELATIVE);
    let source_root_text = source_root
        .to_str()
        .expect("LinuxCNC source path is not valid UTF-8");
    let source_header = source_root.join(SOURCE_HAL_HEADER_RELATIVE);
    println!("cargo:rerun-if-changed={HAL_HEADER}");
    println!("cargo:rerun-if-changed={}", source_header.display());

    let installed_version = process::text("linuxcnc_var", &["LINUXCNCVERSION"]);
    assert_eq!(
        installed_version.trim(),
        EXPECTED_LINUXCNC_VERSION,
        "refusing to compile against an unaudited LinuxCNC version"
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

    let installed = fs::read(installed_header)
        .unwrap_or_else(|error| panic!("failed to read {HAL_HEADER}: {error}"));
    let authoritative = fs::read(&source_header).unwrap_or_else(|error| {
        panic!(
            "failed to read authoritative HAL header {}: {error}",
            source_header.display()
        )
    });
    assert_eq!(
        installed, authoritative,
        "installed hal.h does not exactly match the audited LinuxCNC v2.9.10 source"
    );
}
