use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const MACHINE_CONFIG_RELATIVE: &str = "../../../config/machine-pulses.conf";

fn setting(text: &str, name: &str, machine_config: &Path) -> u32 {
    let prefix = format!("{name}=");
    let values: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix(&prefix))
        .collect();
    assert_eq!(
        values.len(),
        1,
        "expected exactly one {name} in {}",
        machine_config.display()
    );
    values[0]
        .parse::<u32>()
        .unwrap_or_else(|error| panic!("invalid {name} in {}: {error}", machine_config.display()))
}

fn main() {
    let manifest_directory =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let machine_config = manifest_directory.join(MACHINE_CONFIG_RELATIVE);

    println!("cargo:rerun-if-changed={}", machine_config.display());
    println!("cargo:rerun-if-changed=build.rs");
    assert!(
        Path::new(&machine_config).is_file(),
        "missing {}",
        machine_config.display()
    );
    let text = fs::read_to_string(&machine_config)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", machine_config.display()));
    let motor = setting(&text, "MOTOR_PULSES_PER_REV", &machine_config);
    let reference = setting(&text, "REFERENCE_PULSES_PER_REV", &machine_config);
    assert!(
        motor > 0 && reference > 0,
        "pulse settings must be positive"
    );
    assert_eq!(
        motor % reference,
        0,
        "machine pulse scale must be an integer"
    );
    let scale = motor / reference;
    assert_eq!(scale, 5, "refusing an unreviewed machine pulse scale");

    let generated = format!(
        "pub const MOTOR_PULSES_PER_REV: i32 = {motor};\n\
         pub const REFERENCE_PULSES_PER_REV: i32 = {reference};\n\
         pub const MOTOR_PULSE_SCALE: i32 = {scale};\n"
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"))
        .join("machine_scale.rs");
    fs::write(output, generated).expect("failed to write generated machine scale");
}
