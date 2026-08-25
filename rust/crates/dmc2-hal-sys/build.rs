use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const HAL_HEADER: &str = "/usr/include/linuxcnc/hal.h";
const EXPECTED_LINUXCNC_VERSION: &str = "2.9.10";

fn command_output(program: &str, arguments: &[&str]) -> String {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("failed to execute {program}: {error}"));
    assert!(
        output.status.success(),
        "{program} failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("{program} produced non-UTF-8 output: {error}"))
}

fn main() {
    println!("cargo:rerun-if-changed={HAL_HEADER}");
    println!("cargo:rerun-if-changed=build.rs");

    assert!(
        Path::new(HAL_HEADER).is_file(),
        "the installed LinuxCNC HAL header is missing: {HAL_HEADER}"
    );

    let installed_version = command_output("linuxcnc_var", &["LINUXCNCVERSION"]);
    assert_eq!(
        installed_version.trim(),
        EXPECTED_LINUXCNC_VERSION,
        "refusing to compile against an unaudited LinuxCNC version"
    );

    let output_directory =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"));
    let bindings_path = output_directory.join("hal_bindings.rs");
    let bindgen = Command::new("bindgen")
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
        .args(["--", "-DRTAPI", "-I/usr/include/linuxcnc"])
        .output()
        .unwrap_or_else(|error| panic!("failed to execute bindgen: {error}"));
    assert!(
        bindgen.status.success(),
        "bindgen failed with {}: {}",
        bindgen.status,
        String::from_utf8_lossy(&bindgen.stderr)
    );

    let bindings = fs::read_to_string(&bindings_path)
        .unwrap_or_else(|error| panic!("failed to read generated bindings: {error}"));
    for required_symbol in [
        "pub fn hal_init",
        "pub fn hal_exit",
        "pub fn hal_ready",
        "pub fn hal_malloc",
        "pub fn hal_pin_bit_new",
        "pub fn hal_pin_float_new",
        "pub fn hal_pin_s32_new",
        "pub fn hal_pin_u32_new",
        "pub fn hal_export_funct",
        "hal_pin_dir_t_HAL_IN: hal_pin_dir_t = 16",
        "hal_pin_dir_t_HAL_OUT: hal_pin_dir_t = 32",
        "hal_pin_dir_t_HAL_IO: hal_pin_dir_t = 48",
    ] {
        assert!(
            bindings.contains(required_symbol),
            "generated LinuxCNC bindings omitted required ABI item: {required_symbol}"
        );
    }
}
