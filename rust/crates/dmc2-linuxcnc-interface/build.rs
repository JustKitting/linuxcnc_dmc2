use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const VERSION: &str = "2.9.10";
const COMMIT: &str = "86cdca76fa2a36274c432caa21952b23c267989a";
const CATALOG: &str = include_str!("data/linuxcnc-2.9.10-catalog.rsdata");
const SOURCE_RELATIVE: &str = "../../../vendor/linuxcnc-2.9.10";
const INSTALLED_HEADERS: &str = "/usr/include/linuxcnc";

fn command(program: &str, arguments: &[&str]) -> String {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("failed to execute {program}: {error}"));
    assert!(
        output.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("{program} output was not UTF-8: {error}"))
}

fn quoted<'a>(line: &'a str, field: &str) -> &'a str {
    let value = line
        .split_once(field)
        .unwrap_or_else(|| panic!("catalog row is missing {field}: {line}"))
        .1;
    let value = value
        .strip_prefix('"')
        .unwrap_or_else(|| panic!("catalog {field} is not quoted: {line}"));
    value
        .split_once('"')
        .unwrap_or_else(|| panic!("catalog {field} is unterminated: {line}"))
        .0
}

fn decimal(line: &str, field: &str) -> usize {
    line.split_once(field)
        .unwrap_or_else(|| panic!("catalog row is missing {field}: {line}"))
        .1
        .split_once(',')
        .unwrap_or_else(|| panic!("catalog {field} has no delimiter: {line}"))
        .0
        .parse()
        .unwrap_or_else(|error| panic!("catalog {field} is invalid: {error}"))
}

fn hexadecimal(line: &str, field: &str) -> u64 {
    let value = line
        .split_once(field)
        .unwrap_or_else(|| panic!("catalog row is missing {field}: {line}"))
        .1
        .split_once(',')
        .unwrap_or_else(|| panic!("catalog {field} has no delimiter: {line}"))
        .0
        .strip_prefix("0x")
        .unwrap_or_else(|| panic!("catalog {field} is not hexadecimal: {line}"));
    u64::from_str_radix(value, 16)
        .unwrap_or_else(|error| panic!("catalog {field} is invalid: {error}"))
}

fn fnv64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn verify_header(source_root: &Path, row: &str) -> usize {
    let header = quoted(row, "header_name: ");
    let relative = quoted(row, "source_relative_path: ");
    let expected_bytes = decimal(row, "source_byte_count: ");
    let expected_hash = hexadecimal(row, "source_fnv64: ");
    let installed_path = Path::new(INSTALLED_HEADERS).join(header);
    let source_path = source_root.join(relative);
    println!("cargo:rerun-if-changed={}", installed_path.display());
    println!("cargo:rerun-if-changed={}", source_path.display());
    let installed = fs::read(&installed_path).unwrap_or_else(|error| {
        panic!(
            "failed to read installed {}: {error}",
            installed_path.display()
        )
    });
    let source = fs::read(&source_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", source_path.display()));
    assert_eq!(
        installed, source,
        "installed {header} differs from the pinned LinuxCNC source"
    );
    assert_eq!(source.len(), expected_bytes, "{header} byte count changed");
    assert_eq!(
        fnv64(&source),
        expected_hash,
        "{header} fingerprint changed"
    );
    source.len()
}

fn verify_linuxcnc() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing manifest"));
    let source_root = manifest.join(SOURCE_RELATIVE);
    let source_text = source_root
        .to_str()
        .expect("LinuxCNC source path is not UTF-8");
    assert_eq!(
        command("linuxcnc_var", &["LINUXCNCVERSION"]).trim(),
        VERSION,
        "installed LinuxCNC version changed"
    );
    assert_eq!(
        command("git", &["-C", source_text, "rev-parse", "HEAD"]).trim(),
        COMMIT,
        "pinned LinuxCNC source commit changed"
    );
    assert!(
        command("git", &["-C", source_text, "status", "--porcelain"])
            .trim()
            .is_empty(),
        "pinned LinuxCNC source has local modifications"
    );

    let mut headers = 0;
    let mut bytes = 0;
    for row in CATALOG
        .lines()
        .filter(|line| line.starts_with("PublicHeaderContract {"))
    {
        headers += 1;
        bytes += verify_header(&source_root, row);
    }
    assert_eq!(headers, 120, "catalog header inventory changed");
    assert_eq!(bytes, 635_278, "catalog header byte inventory changed");
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=data/linuxcnc-2.9.10-catalog.rsdata");
    verify_linuxcnc();
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("missing OUT_DIR"));
    fs::write(output.join("linuxcnc_code_catalog.rs"), CATALOG)
        .expect("failed to materialize the pinned LinuxCNC catalog");
}
