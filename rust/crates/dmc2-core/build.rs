use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const MACHINE_CONFIG_RELATIVE: &str = "../../../config/machine-pulses.conf";
const LIVE_INI_RELATIVE: &str = "../../../live/dmc2.ini";

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

fn ini_setting(text: &str, section: &str, name: &str, ini_path: &Path) -> f64 {
    let mut current_section = "";
    let mut values = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if let Some(value) = line
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            current_section = value.trim();
            continue;
        }
        if current_section != section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == name {
            values.push(value.trim());
        }
    }
    assert_eq!(
        values.len(),
        1,
        "expected exactly one [{section}]{name} in {}",
        ini_path.display()
    );
    values[0].parse::<f64>().unwrap_or_else(|error| {
        panic!(
            "invalid [{section}]{name} in {}: {error}",
            ini_path.display()
        )
    })
}

fn position_move_seconds(distance: f64, max_velocity: f64, max_acceleration: f64) -> f64 {
    let full_acceleration_distance = max_velocity * max_velocity / max_acceleration;
    if distance <= full_acceleration_distance {
        2.0 * (distance / max_acceleration).sqrt()
    } else {
        2.0 * max_velocity / max_acceleration
            + (distance - full_acceleration_distance) / max_velocity
    }
}

fn nanoseconds(seconds: f64) -> u64 {
    (seconds * 1_000_000_000.0).ceil() as u64
}

fn main() {
    let manifest_directory =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let machine_config = manifest_directory.join(MACHINE_CONFIG_RELATIVE);
    let live_ini = manifest_directory.join(LIVE_INI_RELATIVE);

    println!("cargo:rerun-if-changed={}", machine_config.display());
    println!("cargo:rerun-if-changed={}", live_ini.display());
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

    let pulses_per_mm = setting(&text, "PULSES_PER_MM", &machine_config);
    let increments = [
        setting(&text, "PENDANT_X1_PULSES", &machine_config),
        setting(&text, "PENDANT_X10_PULSES", &machine_config),
        setting(&text, "PENDANT_X100_PULSES", &machine_config),
    ];
    let target_rates = [
        setting(
            &text,
            "PENDANT_X1_TARGET_PULSES_PER_SECOND",
            &machine_config,
        ),
        setting(
            &text,
            "PENDANT_X10_TARGET_PULSES_PER_SECOND",
            &machine_config,
        ),
        setting(
            &text,
            "PENDANT_X100_TARGET_PULSES_PER_SECOND",
            &machine_config,
        ),
    ];
    let bounce_pulses = setting(&text, "BOUNCE_PULSES", &machine_config);
    let bounce_target_rate = setting(&text, "BOUNCE_TARGET_PULSES_PER_SECOND", &machine_config);
    assert_eq!(
        increments,
        [10, 100, 1_000],
        "refusing unreviewed jog distances"
    );
    assert_eq!(
        target_rates,
        [5_000, 75_000, 300_000],
        "refusing unreviewed jog target rates"
    );
    assert_eq!(
        bounce_pulses,
        50 * scale,
        "bounce distance must retain the accepted motor pulse scale"
    );
    assert_eq!(
        bounce_target_rate,
        300 * scale,
        "bounce target rate must retain the accepted motor pulse scale"
    );

    let ini_text = fs::read_to_string(&live_ini)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", live_ini.display()));
    let axis_velocity = ["AXIS_X", "AXIS_Y", "AXIS_Z"]
        .map(|section| ini_setting(&ini_text, section, "MAX_VELOCITY", &live_ini));
    let axis_acceleration = ["AXIS_X", "AXIS_Y", "AXIS_Z"]
        .map(|section| ini_setting(&ini_text, section, "MAX_ACCELERATION", &live_ini));
    let joint_velocity = ["JOINT_0", "JOINT_1", "JOINT_2"]
        .map(|section| ini_setting(&ini_text, section, "MAX_VELOCITY", &live_ini));
    let joint_acceleration = ["JOINT_0", "JOINT_1", "JOINT_2"]
        .map(|section| ini_setting(&ini_text, section, "MAX_ACCELERATION", &live_ini));
    let ini_pulses_per_mm = ini_setting(&ini_text, "DMC2", "PULSES_PER_MM", &live_ini);
    assert_eq!(ini_pulses_per_mm, pulses_per_mm as f64);
    for value in axis_velocity
        .into_iter()
        .chain(axis_acceleration)
        .chain(joint_velocity)
        .chain(joint_acceleration)
    {
        assert!(
            value.is_finite() && value > 0.0,
            "planner limits must be positive"
        );
    }

    let mut max_jog_seconds = 0.0_f64;
    for (distance_pulses, rate_pulses_per_second) in increments.into_iter().zip(target_rates) {
        let distance = distance_pulses as f64 / pulses_per_mm as f64;
        let issue_seconds = distance_pulses as f64 / rate_pulses_per_second as f64;
        for (velocity, acceleration) in axis_velocity
            .into_iter()
            .zip(axis_acceleration)
            .chain(joint_velocity.into_iter().zip(joint_acceleration))
        {
            max_jog_seconds = max_jog_seconds
                .max(issue_seconds + position_move_seconds(distance, velocity, acceleration));
        }
    }
    let bounce_distance = bounce_pulses as f64 / pulses_per_mm as f64;
    let bounce_issue_seconds = bounce_pulses as f64 / bounce_target_rate as f64;
    let max_bounce_seconds = axis_velocity
        .into_iter()
        .zip(axis_acceleration)
        .chain(joint_velocity.into_iter().zip(joint_acceleration))
        .map(|(velocity, acceleration)| {
            bounce_issue_seconds + position_move_seconds(bounce_distance, velocity, acceleration)
        })
        .fold(0.0_f64, f64::max);
    let max_stop_seconds = axis_velocity
        .into_iter()
        .zip(axis_acceleration)
        .chain(joint_velocity.into_iter().zip(joint_acceleration))
        .map(|(velocity, acceleration)| velocity / acceleration)
        .fold(0.0_f64, f64::max);

    const ACCEPTANCE_NS: u64 = 100_000_000;
    const OBSERVATION_MARGIN_NS: u64 = 250_000_000;
    const EXISTING_TIMEOUT_FLOOR_NS: u64 = 2_000_000_000;
    let required_jog_timeout_ns =
        ACCEPTANCE_NS + nanoseconds(max_jog_seconds) + OBSERVATION_MARGIN_NS;
    let required_bounce_timeout_ns =
        ACCEPTANCE_NS + nanoseconds(max_bounce_seconds) + OBSERVATION_MARGIN_NS;
    let required_stop_timeout_ns = nanoseconds(max_stop_seconds) + OBSERVATION_MARGIN_NS;
    let jog_timeout_ns = required_jog_timeout_ns.max(EXISTING_TIMEOUT_FLOOR_NS);
    let bounce_timeout_ns = required_bounce_timeout_ns.max(EXISTING_TIMEOUT_FLOOR_NS);
    let stop_timeout_ns = required_stop_timeout_ns.max(EXISTING_TIMEOUT_FLOOR_NS);

    let generated = format!(
        "pub const MOTOR_PULSES_PER_REV: i32 = {motor};\n\
         pub const REFERENCE_PULSES_PER_REV: i32 = {reference};\n\
         pub const MOTOR_PULSE_SCALE: i32 = {scale};\n\
         pub const PULSES_PER_MM: i32 = {pulses_per_mm};\n\
         pub const PENDANT_INCREMENT_PULSES: [i32; 3] = {increments:?};\n\
         pub const PENDANT_TARGET_PULSES_PER_SECOND: [i32; 3] = {target_rates:?};\n\
         pub const BOUNCE_PULSES: i32 = {bounce_pulses};\n\
         pub const BOUNCE_TARGET_PULSES_PER_SECOND: i32 = {bounce_target_rate};\n\
         pub const AXIS_MAX_VELOCITY_MM_PER_SECOND: [f64; 3] = {axis_velocity:?};\n\
         pub const AXIS_MAX_ACCELERATION_MM_PER_SECOND_SQUARED: [f64; 3] = {axis_acceleration:?};\n\
         pub const JOINT_MAX_VELOCITY_MM_PER_SECOND: [f64; 3] = {joint_velocity:?};\n\
         pub const JOINT_MAX_ACCELERATION_MM_PER_SECOND_SQUARED: [f64; 3] = {joint_acceleration:?};\n\
         pub const REQUIRED_JOG_TIMEOUT_NS: u64 = {required_jog_timeout_ns};\n\
         pub const REQUIRED_BOUNCE_TIMEOUT_NS: u64 = {required_bounce_timeout_ns};\n\
         pub const REQUIRED_MOTION_STOP_TIMEOUT_NS: u64 = {required_stop_timeout_ns};\n\
         pub const CONFIGURED_JOG_TIMEOUT_NS: u64 = {jog_timeout_ns};\n\
         pub const CONFIGURED_BOUNCE_TIMEOUT_NS: u64 = {bounce_timeout_ns};\n\
         pub const CONFIGURED_MOTION_STOP_TIMEOUT_NS: u64 = {stop_timeout_ns};\n"
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"))
        .join("machine_scale.rs");
    fs::write(output, generated).expect("failed to write generated machine scale");
}
