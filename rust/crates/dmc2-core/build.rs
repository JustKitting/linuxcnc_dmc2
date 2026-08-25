use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const MACHINE_CONFIG: &str = "/home/kit/cnc_motion_config.sh";

fn setting(text: &str, name: &str) -> u32 {
    let prefix = format!("{name}=");
    let values: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix(&prefix))
        .collect();
    assert_eq!(
        values.len(),
        1,
        "expected exactly one {name} in {MACHINE_CONFIG}"
    );
    values[0]
        .parse::<u32>()
        .unwrap_or_else(|error| panic!("invalid {name} in {MACHINE_CONFIG}: {error}"))
}

fn main() {
    println!("cargo:rerun-if-changed={MACHINE_CONFIG}");
    println!("cargo:rerun-if-changed=build.rs");
    assert!(
        Path::new(MACHINE_CONFIG).is_file(),
        "missing {MACHINE_CONFIG}"
    );
    let text = fs::read_to_string(MACHINE_CONFIG)
        .unwrap_or_else(|error| panic!("failed to read {MACHINE_CONFIG}: {error}"));
    let motor = setting(&text, "MOTOR_PULSES_PER_REV");
    let reference = setting(&text, "REFERENCE_PULSES_PER_REV");
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
