use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const EXPECTED_LINUXCNC_VERSION: &str = "2.9.10";
const INCLUDE_ROOT: &str = "/usr/include/linuxcnc";
const SOURCE_ROOT_RELATIVE: &str = "../../../vendor/linuxcnc-2.9.10";

fn output(program: &str, arguments: &[&str]) -> std::process::Output {
    Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("failed to execute {program}: {error}"))
}

fn run(program: &str, arguments: &[&str]) {
    let result = output(program, arguments);
    assert!(
        result.status.success(),
        "{program} failed with {}: {}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
}

fn exact_header(manifest: &Path, installed_name: &str, source_relative: &str) {
    let installed = Path::new(INCLUDE_ROOT).join(installed_name);
    let source = manifest.join(SOURCE_ROOT_RELATIVE).join(source_relative);
    println!("cargo:rerun-if-changed={}", installed.display());
    println!("cargo:rerun-if-changed={}", source.display());
    assert_eq!(
        fs::read(&installed)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", installed.display())),
        fs::read(&source)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", source.display())),
        "installed {installed_name} differs from official LinuxCNC 2.9.10 source"
    );
}

fn path_text(path: &Path) -> &str {
    path.to_str().expect("build path is not valid UTF-8")
}

fn main() {
    let manifest =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let native = manifest.join("src/native");
    let header = native.join("control_client.h");
    let source = native.join("control_client.cc");
    let output_directory =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"));

    for path in [&header, &source] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        manifest.join("../../../config/operations.tsv").display()
    );

    let version = output("linuxcnc_var", &["LINUXCNCVERSION"]);
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
        "refusing to compile dmc2ctl against an unaudited LinuxCNC version"
    );

    for (installed, official) in [
        ("cms.hh", "src/libnml/cms/cms.hh"),
        ("cmd_msg.hh", "src/libnml/nml/cmd_msg.hh"),
        ("emc.hh", "src/emc/nml_intf/emc.hh"),
        ("emc_nml.hh", "src/emc/nml_intf/emc_nml.hh"),
        ("emcpos.h", "src/emc/nml_intf/emcpos.h"),
        ("emctool.h", "src/emc/nml_intf/emctool.h"),
        ("nml.hh", "src/libnml/nml/nml.hh"),
        ("nml_type.hh", "src/libnml/nml/nml_type.hh"),
        ("rcs.hh", "src/libnml/rcs/rcs.hh"),
        ("stat_msg.hh", "src/libnml/nml/stat_msg.hh"),
    ] {
        exact_header(&manifest, installed, official);
    }

    let bindings = output_directory.join("control_client_bindings.rs");
    run(
        "bindgen",
        &[
            path_text(&header),
            "--allowlist-type",
            "^dmc2_.*",
            "--allowlist-function",
            "^dmc2_.*",
            "--allowlist-var",
            "^DMC2_.*",
            "--use-core",
            "--with-derive-default",
            "--with-derive-partialeq",
            "--no-prepend-enum-name",
            "--formatter",
            "rustfmt",
            "--output",
            path_text(&bindings),
            "--",
            "-x",
            "c",
            "-std=c11",
        ],
    );

    let object = output_directory.join("control_client.o");
    run(
        "g++",
        &[
            "-std=c++17",
            "-O2",
            "-fPIC",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-isystem",
            INCLUDE_ROOT,
            "-I",
            path_text(&native),
            "-c",
            path_text(&source),
            "-o",
            path_text(&object),
        ],
    );

    let archive = output_directory.join("libdmc2_control_native.a");
    if archive.exists() {
        fs::remove_file(&archive).unwrap_or_else(|error| {
            panic!("failed to remove stale {}: {error}", archive.display())
        });
    }
    run("ar", &["crus", path_text(&archive), path_text(&object)]);

    println!(
        "cargo:rustc-link-search=native={}",
        output_directory.display()
    );
    println!("cargo:rustc-link-search=native=/usr/lib");
    println!("cargo:rustc-link-lib=static=dmc2_control_native");
    println!("cargo:rustc-link-lib=static=linuxcnc");
    println!("cargo:rustc-link-lib=tooldata");
    println!("cargo:rustc-link-lib=nml");
    println!("cargo:rustc-link-lib=linuxcnchal");
    println!("cargo:rustc-link-lib=stdc++");
}
