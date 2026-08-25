use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const EXPECTED_LINUXCNC_VERSION: &str = "2.9.10";
const INSTALLED_EMC_NML: &str = "/usr/include/linuxcnc/emc_nml.hh";
const SOURCE_EMC_NML_RELATIVE: &str = "../../../vendor/linuxcnc-2.9.10/src/emc/nml_intf/emc_nml.hh";

fn run(program: &str, arguments: &[&str]) {
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
}

fn main() {
    let manifest_directory =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let source_emc_nml = manifest_directory.join(SOURCE_EMC_NML_RELATIVE);

    println!("cargo:rerun-if-changed=src/task_status_shim.cc");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={INSTALLED_EMC_NML}");
    println!("cargo:rerun-if-changed={}", source_emc_nml.display());

    let version = Command::new("linuxcnc_var")
        .arg("LINUXCNCVERSION")
        .output()
        .expect("failed to execute linuxcnc_var");
    assert!(
        version.status.success(),
        "linuxcnc_var failed: {}",
        String::from_utf8_lossy(&version.stderr)
    );
    assert_eq!(
        String::from_utf8(version.stdout)
            .expect("linuxcnc_var produced non-UTF-8 output")
            .trim(),
        EXPECTED_LINUXCNC_VERSION,
        "refusing to compile task monitor against an unaudited LinuxCNC version"
    );
    assert_eq!(
        fs::read(INSTALLED_EMC_NML).expect("failed to read installed emc_nml.hh"),
        fs::read(&source_emc_nml).expect("failed to read source emc_nml.hh"),
        "installed emc_nml.hh differs from the official LinuxCNC 2.9.10 source"
    );

    let output_directory =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"));
    let object = output_directory.join("task_status_shim.o");
    let archive = output_directory.join("libdmc2_task_status_shim.a");
    let object_text = object
        .to_str()
        .expect("Cargo output path is not valid UTF-8");
    let archive_text = archive
        .to_str()
        .expect("Cargo output path is not valid UTF-8");

    run(
        "g++",
        &[
            "-std=c++17",
            "-O2",
            "-fPIC",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-Wno-error=ignored-qualifiers",
            "-isystem",
            "/usr/include/linuxcnc",
            "-c",
            "src/task_status_shim.cc",
            "-o",
            object_text,
        ],
    );
    run("ar", &["crus", archive_text, object_text]);

    println!(
        "cargo:rustc-link-search=native={}",
        output_directory.display()
    );
    println!("cargo:rustc-link-search=native=/usr/lib");
    println!("cargo:rustc-link-lib=static=dmc2_task_status_shim");
    println!("cargo:rustc-link-lib=static=linuxcnc");
    println!("cargo:rustc-link-lib=nml");
    println!("cargo:rustc-link-lib=linuxcnchal");
    println!("cargo:rustc-link-lib=stdc++");
}
