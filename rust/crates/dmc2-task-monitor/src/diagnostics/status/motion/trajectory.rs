//! EMC_TRAJ_STAT validation.

use dmc2_linuxcnc_interface::{
    EMCMOT_MAX_AXIS, EMCMOT_MAX_JOINTS, EMCMOT_MAX_SPINDLES, KINEMATICS_TYPE, MOTION_TYPE,
    STATE_TAG_FLAG, TRAJ_MODE,
};

use crate::snapshot::NativeSnapshot;

use super::super::super::catalog::{
    check_code, check_code_with_sentinels, check_i32_set, domain_id, issue,
};
use super::super::super::category;
use super::super::super::report::{DiagnosticReport, Severity};
use super::super::super::validation::{
    account_open, check_finite, check_finite_f32, check_i32_range, check_pose, check_u32_flag,
};

pub(super) fn evaluate(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    let trajectory = &snapshot.trajectory;
    check_finite(report, "trajectory.linear_units", trajectory.linear_units);
    check_finite(report, "trajectory.angular_units", trajectory.angular_units);
    check_finite(report, "trajectory.cycle_time", trajectory.cycle_time);
    check_i32_range(
        report,
        "trajectory.joints",
        trajectory.joints,
        1,
        EMCMOT_MAX_JOINTS as i32,
        "configured_count",
        "configured joint count is outside LinuxCNC 2.9.10 bounds",
    );
    check_i32_range(
        report,
        "trajectory.spindles",
        trajectory.spindles,
        0,
        EMCMOT_MAX_SPINDLES as i32,
        "configured_count",
        "configured spindle count is outside LinuxCNC 2.9.10 bounds",
    );

    report.account("trajectory.axis_mask", "bounded_axis_bitmask");
    let valid_axis_mask = (1_i32 << EMCMOT_MAX_AXIS) - 1;
    if trajectory.axis_mask & !valid_axis_mask != 0 {
        issue(
            report,
            Severity::Error,
            category::INVALID_VALUE,
            "trajectory.axis_mask",
            "axis_mask",
            u32::MAX,
            i64::from(trajectory.axis_mask),
            None,
            "axis mask contains bits beyond LinuxCNC 2.9.10 maximum axes",
        );
    }
    check_code(
        report,
        "trajectory.mode",
        TRAJ_MODE,
        i64::from(trajectory.mode),
    );
    check_u32_flag(
        report,
        "trajectory.enabled",
        trajectory.enabled,
        "trajectory enabled state is neither false nor true",
    );
    check_u32_flag(
        report,
        "trajectory.in_position",
        trajectory.in_position,
        "trajectory in-position state is neither false nor true",
    );
    check_i32_range(
        report,
        "trajectory.queue",
        trajectory.queue,
        0,
        i32::MAX,
        "nonnegative_count",
        "trajectory queue depth is negative",
    );
    check_i32_range(
        report,
        "trajectory.active_queue",
        trajectory.active_queue,
        -1,
        i32::MAX,
        "linuxcnc_2_9_10_active_depth",
        "active trajectory queue depth is below LinuxCNC 2.9.10's minimum value of -1",
    );
    check_u32_flag(
        report,
        "trajectory.queue_full",
        trajectory.queue_full,
        "trajectory queue-full state is neither false nor true",
    );
    account_open(report, "trajectory.id", "open_motion_identifier");
    check_u32_flag(
        report,
        "trajectory.paused",
        trajectory.paused,
        "trajectory paused state is neither false nor true",
    );
    check_finite(report, "trajectory.scale", trajectory.scale);
    check_finite(report, "trajectory.rapid_scale", trajectory.rapid_scale);
    check_pose(report, "trajectory.position", trajectory.position);
    check_pose(
        report,
        "trajectory.actual_position",
        trajectory.actual_position,
    );
    check_finite(report, "trajectory.velocity", trajectory.velocity);
    check_finite(report, "trajectory.acceleration", trajectory.acceleration);
    check_finite(report, "trajectory.max_velocity", trajectory.max_velocity);
    check_finite(
        report,
        "trajectory.max_acceleration",
        trajectory.max_acceleration,
    );
    check_pose(
        report,
        "trajectory.probed_position",
        trajectory.probed_position,
    );
    check_u32_flag(
        report,
        "trajectory.probe_tripped",
        trajectory.probe_tripped,
        "probe-tripped state is neither false nor true",
    );
    check_u32_flag(
        report,
        "trajectory.probing",
        trajectory.probing,
        "probing state is neither false nor true",
    );
    check_i32_set(
        report,
        "trajectory.probe_value",
        trajectory.probe_value,
        &[0, 1],
        "probe input status is neither low nor high",
    );
    check_code_with_sentinels(
        report,
        "trajectory.kinematics_type",
        KINEMATICS_TYPE,
        i64::from(trajectory.kinematics_type),
        &[0],
    );
    check_code_with_sentinels(
        report,
        "trajectory.motion_type",
        MOTION_TYPE,
        i64::from(trajectory.motion_type),
        &[0],
    );
    check_finite(
        report,
        "trajectory.distance_to_go",
        trajectory.distance_to_go,
    );
    check_pose(report, "trajectory.dtg", trajectory.dtg);
    check_finite(
        report,
        "trajectory.current_velocity",
        trajectory.current_velocity,
    );
    check_u32_flag(
        report,
        "trajectory.feed_override_enabled",
        trajectory.feed_override_enabled,
        "feed-override state is neither false nor true",
    );
    check_u32_flag(
        report,
        "trajectory.adaptive_feed_enabled",
        trajectory.adaptive_feed_enabled,
        "adaptive-feed state is neither false nor true",
    );
    check_u32_flag(
        report,
        "trajectory.feed_hold_enabled",
        trajectory.feed_hold_enabled,
        "feed-hold state is neither false nor true",
    );

    account_open(
        report,
        "trajectory.state_tag.fields_float[0]",
        "linuxcnc_2_9_10_unused_float_line_number_slot",
    );
    for (index, value) in trajectory
        .state_tag
        .fields_float
        .iter()
        .copied()
        .enumerate()
        .skip(1)
    {
        check_finite_f32(
            report,
            format!("trajectory.state_tag.fields_float[{index}]"),
            value,
        );
    }
    account_open(
        report,
        "trajectory.state_tag.fields",
        "open_state_tag_integer_array",
    );
    report.account(
        "trajectory.state_tag.packed_flags",
        "source_state_tag_bitmask",
    );
    let state_tag_flag_count = STATE_TAG_FLAG
        .codes
        .iter()
        .find(|code| code.name == "GM_FLAG_MAX_FLAGS")
        .map(|code| code.code);
    let valid_state_tag_flags = match state_tag_flag_count {
        Some(count) if (0..64).contains(&count) => (1_u64 << count) - 1,
        Some(64) => u64::MAX,
        observed => {
            issue(
                report,
                Severity::Error,
                category::DIAGNOSTIC_INTERFACE,
                "trajectory.state_tag.flag_contract",
                STATE_TAG_FLAG.name,
                domain_id(STATE_TAG_FLAG),
                observed.unwrap_or(-1),
                None,
                "the pinned LinuxCNC interface catalog omitted or invalidated GM_FLAG_MAX_FLAGS",
            );
            0
        }
    };
    if trajectory.state_tag.packed_flags & !valid_state_tag_flags != 0 {
        issue(
            report,
            Severity::Error,
            category::UNKNOWN_CODE,
            "trajectory.state_tag.packed_flags",
            STATE_TAG_FLAG.name,
            domain_id(STATE_TAG_FLAG),
            (trajectory.state_tag.packed_flags & !valid_state_tag_flags) as i64,
            None,
            "state tag contains flag bits absent from LinuxCNC 2.9.10",
        );
        report.unknown_domain_mask |= 1_u64 << domain_id(STATE_TAG_FLAG);
    }
}
