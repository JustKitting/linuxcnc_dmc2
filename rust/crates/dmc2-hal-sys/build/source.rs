use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use super::config::{
    EXPECTED_LINUXCNC_COMMIT, EXPECTED_LINUXCNC_VERSION, HAL_HEADER, RTAPI_ERRNO_HEADER,
    RTAPI_HEADER, SOURCE_HAL_HEADER_RELATIVE, SOURCE_HAL_LIBRARY_RELATIVE, SOURCE_ROOT_RELATIVE,
    SOURCE_RTAPI_ERRNO_HEADER_RELATIVE, SOURCE_RTAPI_HEADER_RELATIVE,
};
use super::process;

pub(crate) fn verify_audited_source() {
    for installed_header in [HAL_HEADER, RTAPI_HEADER, RTAPI_ERRNO_HEADER] {
        assert!(
            Path::new(installed_header).is_file(),
            "the installed LinuxCNC interface header is missing: {installed_header}"
        );
    }

    let source_root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"))
            .join(SOURCE_ROOT_RELATIVE);
    let source_root_text = source_root
        .to_str()
        .expect("LinuxCNC source path is not valid UTF-8");
    let audited_headers = [
        (HAL_HEADER, source_root.join(SOURCE_HAL_HEADER_RELATIVE)),
        (RTAPI_HEADER, source_root.join(SOURCE_RTAPI_HEADER_RELATIVE)),
        (
            RTAPI_ERRNO_HEADER,
            source_root.join(SOURCE_RTAPI_ERRNO_HEADER_RELATIVE),
        ),
    ];
    println!(
        "cargo:rerun-if-changed={}",
        source_root.join(SOURCE_HAL_LIBRARY_RELATIVE).display()
    );
    for (installed, authoritative) in &audited_headers {
        println!("cargo:rerun-if-changed={installed}");
        println!("cargo:rerun-if-changed={}", authoritative.display());
    }

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

    for (installed_path, authoritative_path) in audited_headers {
        let installed = fs::read(installed_path)
            .unwrap_or_else(|error| panic!("failed to read {installed_path}: {error}"));
        let authoritative = fs::read(&authoritative_path).unwrap_or_else(|error| {
            panic!(
                "failed to read authoritative header {}: {error}",
                authoritative_path.display()
            )
        });
        assert_eq!(
            installed, authoritative,
            "installed {installed_path} does not exactly match the audited LinuxCNC v2.9.10 source"
        );
    }
}
