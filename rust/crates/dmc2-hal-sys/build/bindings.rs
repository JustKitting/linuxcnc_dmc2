use std::path::Path;
use std::process::Command;

use super::config::{HAL_HEADER, INCLUDE_ROOT};
use super::process;

pub(crate) fn generate(output_directory: &Path) {
    let bindings_path = output_directory.join("hal_bindings.rs");
    process::run(
        Command::new("bindgen")
            .args([
                HAL_HEADER,
                "--allowlist-function",
                "hal_(init|exit|ready|malloc|pin_(bit|float|s32|u32)_new|export_funct)",
                "--allowlist-type",
                "hal_(bit|float|s32|u32|pin_dir)_t",
                "--allowlist-var",
                "HAL_(IN|OUT|IO)",
                "--use-core",
                "--no-layout-tests",
                "--output",
            ])
            .arg(&bindings_path)
            .args(["--", "-DRTAPI", "-I", INCLUDE_ROOT]),
        "bindgen for the audited LinuxCNC HAL header",
    );
}
