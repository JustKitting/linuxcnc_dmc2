use std::env;
use std::path::PathBuf;
use std::process::Command;

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
    println!("cargo:rerun-if-changed=src/serial_shim.c");
    println!("cargo:rerun-if-changed=build.rs");
    let output_directory =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"));
    let object = output_directory.join("serial_shim.o");
    let archive = output_directory.join("libdmc2_serial_shim.a");
    let object_text = object
        .to_str()
        .expect("Cargo output path is not valid UTF-8");
    let archive_text = archive
        .to_str()
        .expect("Cargo output path is not valid UTF-8");
    run(
        "cc",
        &[
            "-std=c11",
            "-O2",
            "-fPIC",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-c",
            "src/serial_shim.c",
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
    println!("cargo:rustc-link-lib=static=dmc2_serial_shim");
    println!("cargo:rustc-link-lib=linuxcnchal");
}
