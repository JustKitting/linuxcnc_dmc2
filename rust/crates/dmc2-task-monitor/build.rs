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
    println!("cargo:rerun-if-changed=src/task_status_shim.cc");
    println!("cargo:rerun-if-changed=build.rs");

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
