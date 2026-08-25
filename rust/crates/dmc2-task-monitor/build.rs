use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "build/status_schema.rs"]
mod status_schema;

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
        "installed {installed_name} differs from the official LinuxCNC 2.9.10 source"
    );
}

fn path_text(path: &Path) -> &str {
    path.to_str().expect("build path is not valid UTF-8")
}

fn main() {
    let manifest =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let native = manifest.join("src/native");
    let snapshot_header = native.join("status_snapshot.h");
    let output_directory =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"));

    const NATIVE_SOURCES: &[&str] = &[
        "status_copy.cc",
        "status_channel.cc",
        "tests/status_fixture_common.cc",
        "tests/status_fixture_task.cc",
        "tests/status_fixture_trajectory.cc",
        "tests/status_fixture_motion_components.cc",
        "tests/status_fixture_io.cc",
        "tests/status_copy_fixture.cc",
    ];

    for path in [
        manifest.join("build.rs"),
        manifest.join("build/status_schema.rs"),
        snapshot_header.clone(),
        native.join("status_copy.hh"),
        native.join("tests/status_fixture.hh"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    for source_name in NATIVE_SOURCES {
        println!(
            "cargo:rerun-if-changed={}",
            native.join(source_name).display()
        );
    }

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
        "refusing to compile task monitor against an unaudited LinuxCNC version"
    );

    for (installed, source) in [
        ("emc_nml.hh", "src/emc/nml_intf/emc_nml.hh"),
        ("emcpos.h", "src/emc/nml_intf/emcpos.h"),
        ("emctool.h", "src/emc/nml_intf/emctool.h"),
        ("linuxcnc.h", "src/emc/linuxcnc.h"),
        ("emcmotcfg.h", "src/emc/motion/emcmotcfg.h"),
        ("motion.h", "src/emc/motion/motion.h"),
        ("state_tag.h", "src/emc/motion/state_tag.h"),
        ("stat_msg.hh", "src/libnml/nml/stat_msg.hh"),
        ("nmlmsg.hh", "src/libnml/nml/nmlmsg.hh"),
        ("nml_type.hh", "src/libnml/nml/nml_type.hh"),
    ] {
        exact_header(&manifest, installed, source);
    }

    let bindings = output_directory.join("status_snapshot_bindings.rs");
    run(
        "bindgen",
        &[
            path_text(&snapshot_header),
            "--allowlist-type",
            "^dmc2_.*",
            "--allowlist-function",
            "^dmc2_.*",
            "--allowlist-var",
            "^DMC2_.*",
            "--use-core",
            "--with-derive-default",
            "--with-derive-partialeq",
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
    status_schema::generate(&snapshot_header, &output_directory);

    let mut objects = Vec::new();
    for source_name in NATIVE_SOURCES {
        let source = native.join(source_name);
        let object = output_directory.join(format!("{}.o", source_name.replace('/', "_")));
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
        objects.push(object);
    }

    let archive = output_directory.join("libdmc2_task_status_native.a");
    if archive.exists() {
        fs::remove_file(&archive).unwrap_or_else(|error| {
            panic!(
                "failed to remove stale native archive {}: {error}",
                archive.display()
            )
        });
    }
    let mut archive_arguments = vec!["crus", path_text(&archive)];
    archive_arguments.extend(objects.iter().map(|path| path_text(path)));
    run("ar", &archive_arguments);

    println!(
        "cargo:rustc-link-search=native={}",
        output_directory.display()
    );
    println!("cargo:rustc-link-search=native=/usr/lib");
    println!("cargo:rustc-link-lib=static=dmc2_task_status_native");
    println!("cargo:rustc-link-lib=static=linuxcnc");
    println!("cargo:rustc-link-lib=tooldata");
    println!("cargo:rustc-link-lib=nml");
    println!("cargo:rustc-link-lib=linuxcnchal");
    println!("cargo:rustc-link-lib=stdc++");
}
