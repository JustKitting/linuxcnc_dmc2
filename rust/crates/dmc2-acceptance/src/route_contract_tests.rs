const MACHINE_HAL: &str = include_str!("../../../../live/machine.hal");
const SPINDLE_TEST: &str = include_str!("../../../../live/nc_files/dmc2_spindle_test.ngc");
const HARDWOOD_ROUTE: &str = include_str!("../../../../live/nc_files/log-top-25mm-hardwood.ngc");

fn executable_lines(program: &str) -> Vec<&str> {
    program
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('(') && *line != "%")
        .collect()
}

fn line_position(lines: &[&str], exact: &str) -> usize {
    lines
        .iter()
        .position(|line| *line == exact)
        .unwrap_or_else(|| panic!("required executable line is missing: {exact}"))
}

#[test]
fn live_hal_exposes_feedback_validated_clockwise_state_to_m66_p4() {
    assert!(MACHINE_HAL.contains("num_dio=5"));
    assert!(MACHINE_HAL.contains(
        "net dmc2-spindle-clockwise-running h100-spindle.forward-running => motion.digital-in-04"
    ));
}

#[test]
fn live_hal_uses_hm2_modbus_consecutive_failure_policy() {
    assert!(MACHINE_HAL.contains("net dmc2-h100-transient-communication-fault hm2_modbus.0.fault"));
    assert!(MACHINE_HAL.contains("setp h100-spindle.link-fault false"));
    assert!(!MACHINE_HAL.contains("hm2_modbus.0.fault => h100-spindle.link-fault"));

    for command in 0..13 {
        let command = format!("hm2_modbus.0.command.{command:02}.disabled");
        assert!(
            MACHINE_HAL.contains(&command),
            "live HAL does not monitor {command}"
        );
    }
}

#[test]
fn hardwood_cut_cannot_reach_motion_before_clockwise_and_speed_confirmation() {
    let lines = executable_lines(HARDWOOD_ROUTE);
    let m3 = line_position(&lines, "S#<spindle_rpm> M3");
    let clockwise = line_position(&lines, "M66 P4 L3 Q#<spindle_transition_timeout_seconds>");
    let at_speed = line_position(&lines, "M66 P2 L3 Q#<spindle_transition_timeout_seconds>");
    let first_motion = lines
        .iter()
        .position(|line| line.starts_with("G53 G1"))
        .expect("hardwood route must contain cutting motion");

    assert!(m3 < clockwise);
    assert!(clockwise < at_speed);
    assert!(at_speed < first_motion);
    assert!(!lines
        .iter()
        .flat_map(|line| line.split_whitespace())
        .any(|word| word == "M4"));
    assert!(HARDWOOD_ROUTE
        .contains("(abort,DMC2 LOG CUT FAILED: H100 did not confirm clockwise rotation)"));
}

#[test]
fn hardwood_resume_starts_at_layer_two_and_stays_inside_y_soft_limits() {
    assert!(HARDWOOD_ROUTE.contains("#<y_home> = 173.0"));
    assert!(HARDWOOD_ROUTE.contains("#<y_min> = 0.01"));
    assert!(HARDWOOD_ROUTE.contains("#<y_max> = 172.99"));
    assert!(HARDWOOD_ROUTE.contains("#<completed_rough_passes> = 1"));
    assert!(HARDWOOD_ROUTE
        .contains("#<depth_removed> = [#<completed_rough_passes> * #<rough_stepdown>]"));
    assert!(HARDWOOD_ROUTE.contains("#<current_y> = #<y_min>"));
    assert!(HARDWOOD_ROUTE.contains("#<y_direction> = 1.0"));

    let x_position = HARDWOOD_ROUTE
        .find("G53 G1 X#<x_max> F#<feed_mm_min>")
        .expect("resume route must position X at the pass-2 start corner");
    let y_position = HARDWOOD_ROUTE
        .find("G53 G1 Y#<y_min> F#<feed_mm_min>")
        .expect("resume route must position Y at the pass-2 start corner");
    let pass_loop = HARDWOOD_ROUTE
        .find("o<dmc2_log_passes> while")
        .expect("resume route must retain the pass loop");
    assert!(y_position < x_position);
    assert!(x_position < pass_loop);
}

#[test]
fn reusable_clockwise_test_uses_direction_feedback_and_commands_no_axis_motion() {
    let lines = executable_lines(SPINDLE_TEST);
    let m3 = line_position(&lines, "S#<test_rpm> M3");
    let clockwise = line_position(&lines, "M66 P4 L3 Q#<transition_timeout_seconds>");
    let at_speed = line_position(&lines, "M66 P2 L3 Q#<transition_timeout_seconds>");

    assert!(m3 < clockwise);
    assert!(clockwise < at_speed);
    assert!(!lines.iter().any(|line| {
        line.starts_with("G0")
            || line.starts_with("G1")
            || line.starts_with("G2")
            || line.starts_with("G3")
    }));
}
