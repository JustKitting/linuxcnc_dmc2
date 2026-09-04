const MACHINE_HAL: &str = include_str!("../../../../live/machine.hal");
const MESA_STATUS_HAL: &str = include_str!("../../../../live/hal/mesa_status_sources.hal");
const LIVE_INI: &str = include_str!("../../../../live/dmc2.ini");
const OPERATIONS: &str = include_str!("../../../../config/operations.tsv");
const SIDE_PROBE_X: &str = include_str!("../../../../live/nc_files/probe-offset-x-side-touch.ngc");
const ABORT_HANDLER: &str = include_str!("../../../../live/nc_files/dmc2_abort.ngc");
const SPINDLE_TEST: &str = include_str!("../../../../live/nc_files/dmc2_spindle_test.ngc");
const HARDWOOD_ROUTE: &str = include_str!("../../../../live/nc_files/log-top-25mm-hardwood.ngc");

fn executable_lines(program: &str) -> Vec<&str> {
    program
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && (!line.starts_with('(') || line.starts_with("(PROBE"))
                && *line != "%"
        })
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
    assert!(MACHINE_HAL.contains("num_dio=6"));
    assert!(MACHINE_HAL.contains(
        "net dmc2-spindle-clockwise-running h100-spindle.forward-running => motion.digital-in-04"
    ));
}

#[test]
fn x_side_probe_selects_in1_before_motion_and_restores_in0_after_backoff() {
    assert!(LIVE_INI.contains("NO_FORCE_HOMING = 1"));
    assert!(MACHINE_HAL.contains("dmc2-probe-select-in0"));
    assert!(MACHINE_HAL.contains("dmc2-probe-select-in1"));
    assert!(MACHINE_HAL.contains("dmc2-selected-probe-or"));
    assert!(MESA_STATUS_HAL.contains("net dmc2-probe-live   => dmc2-probe-select-in1.in0"));
    assert!(MESA_STATUS_HAL
        .contains("net dmc2-selected-probe-live dmc2-selected-probe-or.out => motion.probe-input"));
    assert!(MESA_STATUS_HAL.contains(
        "net dmc2-side-probe-selected motion.digital-out-01 => dmc2-probe-select-in0-enable.in dmc2-probe-select-in1.in1 motion.digital-in-05"
    ));

    let lines = executable_lines(SIDE_PROBE_X);
    let select = line_position(&lines, "M64 P1");
    let select_ack = line_position(&lines, "M66 P5 L3 Q1.0");
    let power = line_position(&lines, "M64 P0");
    let probe = line_position(
        &lines,
        "G38.2 X[-#<maximum_travel_mm>] F#<approach_feed_mm_min>",
    );
    let capture = line_position(&lines, "#<probe_contact_work_x> = #5061");
    let machine_conversion = line_position(
        &lines,
        "#<probe_contact_machine_x> = [#5061 + #5021 - #5420]",
    );
    let close = line_position(&lines, "(PROBECLOSE)");
    let release = line_position(
        &lines,
        "G38.5 X#<backoff_distance_mm> F#<backoff_feed_mm_min>",
    );
    let power_off = lines
        .iter()
        .enumerate()
        .skip(release + 1)
        .find_map(|(index, line)| (*line == "M65 P0").then_some(index))
        .expect("probe power must turn off after contact release");
    let backoff_target = line_position(
        &lines,
        "#<backoff_target_work_x> = [#<probe_contact_work_x> + #<backoff_distance_mm>]",
    );
    let backoff = line_position(
        &lines,
        "G1 X#<backoff_target_work_x> F#<backoff_feed_mm_min>",
    );
    let final_deselect = lines
        .iter()
        .rposition(|line| *line == "M65 P1")
        .expect("side-probe program must restore IN0");

    assert!(select < select_ack);
    assert!(select_ack < power);
    assert!(power < probe);
    assert!(probe < capture);
    assert!(capture < machine_conversion);
    assert!(machine_conversion < close);
    assert!(close < release);
    assert!(release < power_off);
    assert!(power_off < final_deselect);
    assert!(final_deselect < backoff_target);
    assert!(backoff_target < backoff);
    assert!(SIDE_PROBE_X.contains("#<maximum_travel_mm> = 40.00"));
    assert!(SIDE_PROBE_X.contains("#<approach_feed_mm_min> = 6.00"));
    assert!(SIDE_PROBE_X.contains("#<backoff_distance_mm> = 1.00"));
    assert!(SIDE_PROBE_X.contains("#<backoff_feed_mm_min> = 6.00"));
    assert!(ABORT_HANDLER.contains("M65 P0"));
    assert!(ABORT_HANDLER.contains("M65 P1"));

    let operation = OPERATIONS
        .lines()
        .find(|line| line.starts_with("program.probe-offset-x-side-touch\t"))
        .expect("side-probe operation must be cataloged");
    assert!(!operation
        .split('\t')
        .last()
        .unwrap_or_default()
        .contains("all-homed"));
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
fn hardwood_resume_starts_at_rough_pass_four_and_stays_inside_y_soft_limits() {
    assert!(HARDWOOD_ROUTE.contains("#<y_home> = 173.0"));
    assert!(HARDWOOD_ROUTE.contains("#<y_min> = 0.01"));
    assert!(HARDWOOD_ROUTE.contains("#<y_max> = 172.99"));
    assert!(HARDWOOD_ROUTE.contains("#<completed_rough_passes> = 3"));
    assert!(HARDWOOD_ROUTE
        .contains("#<depth_removed> = [#<completed_rough_passes> * #<rough_stepdown>]"));
    assert!(HARDWOOD_ROUTE.contains("#<current_y> = #<y_min>"));
    assert!(HARDWOOD_ROUTE.contains("#<y_direction> = 1.0"));

    let x_position = HARDWOOD_ROUTE
        .find("G53 G1 X#<x_max> F#<feed_mm_min>")
        .expect("resume route must position X at the pass-4 start corner");
    let y_position = HARDWOOD_ROUTE
        .find("G53 G1 Y#<y_min> F#<feed_mm_min>")
        .expect("resume route must position Y at the pass-4 start corner");
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
