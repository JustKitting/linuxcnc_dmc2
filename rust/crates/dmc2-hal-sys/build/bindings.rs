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
                "(hal_(init|exit|ready|malloc|pin_(bit|float|s32|u32)_new|param_(float|u32)_new|param_(bit|float)_set|export_funct|stream_.*)|rtapi_print_msg)",
                "--allowlist-type",
                "(hal_(bit|float|s32|u32|pin_dir|param_dir)_t|msg_level_t)",
                "--allowlist-var",
                "(HAL_(IN|OUT|IO|RW|NAME_LEN)|RTAPI_MSG_ERR|EPERM|ENOMEM|EINVAL)",
                "--use-core",
                "--no-layout-tests",
                "--output",
            ])
            .arg(&bindings_path)
            .args(["--", "-DRTAPI", "-I", INCLUDE_ROOT]),
        "bindgen for the audited LinuxCNC HAL header",
    );
}
