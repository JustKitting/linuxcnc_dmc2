//! Trajectory, joint, axis, spindle, and motion-I/O status validation.

use dmc2_linuxcnc_interface::{
    EMCMOT_MAX_AXIS, EMCMOT_MAX_JOINTS, EMCMOT_MAX_SPINDLES, JOINT_TYPE, KINEMATICS_TYPE,
    MOTION_TYPE, SPINDLE_ORIENT_STATE, STATE_TAG_FLAG, TRAJ_MODE,
};

use crate::snapshot::NativeSnapshot;

use super::super::catalog::{check_code, check_i32_set, domain_id, issue};
use super::super::category;
use super::super::report::{DiagnosticReport, Severity};
use super::rcs::{check_rcs, CommandDomain};

pub(super) fn evaluate(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    evaluate_trajectory(snapshot, report);
    evaluate_joints(snapshot, report);
    evaluate_axes(snapshot, report);
    evaluate_spindles(snapshot, report);
    evaluate_misc_errors(snapshot, report);
}

fn evaluate_trajectory(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    let joint_count = snapshot.trajectory.joints;
    if !(1..=EMCMOT_MAX_JOINTS as i32).contains(&joint_count) {
        issue(
            report,
            Severity::Error,
            category::INVALID_VALUE,
            "trajectory.joints",
            "configured_count",
            u32::MAX,
            i64::from(joint_count),
            None,
            "configured joint count is outside LinuxCNC 2.9.10 bounds",
        );
    }
    let spindle_count = snapshot.trajectory.spindles;
    if !(0..=EMCMOT_MAX_SPINDLES as i32).contains(&spindle_count) {
        issue(
            report,
            Severity::Error,
            category::INVALID_VALUE,
            "trajectory.spindles",
            "configured_count",
            u32::MAX,
            i64::from(spindle_count),
            None,
            "configured spindle count is outside LinuxCNC 2.9.10 bounds",
        );
    }
    let valid_axis_mask = (1_i32 << EMCMOT_MAX_AXIS) - 1;
    if snapshot.trajectory.axis_mask & !valid_axis_mask != 0 {
        issue(
            report,
            Severity::Error,
            category::INVALID_VALUE,
            "trajectory.axis_mask",
            "axis_mask",
            u32::MAX,
            i64::from(snapshot.trajectory.axis_mask),
            None,
            "axis mask contains bits beyond LinuxCNC 2.9.10 maximum axes",
        );
    }
    check_code(
        report,
        "trajectory.mode",
        TRAJ_MODE,
        i64::from(snapshot.trajectory.mode),
    );
    if snapshot.trajectory.kinematics_type != 0 {
        check_code(
            report,
            "trajectory.kinematics_type",
            KINEMATICS_TYPE,
            i64::from(snapshot.trajectory.kinematics_type),
        );
    }
    if snapshot.trajectory.motion_type != 0 {
        check_code(
            report,
            "trajectory.motion_type",
            MOTION_TYPE,
            i64::from(snapshot.trajectory.motion_type),
        );
    }
    check_i32_set(
        report,
        "trajectory.probe_value",
        snapshot.trajectory.probe_value,
        &[0, 1],
        "probe input status is neither low nor high",
    );
    let state_tag_flag_count = STATE_TAG_FLAG
        .codes
        .iter()
        .find(|code| code.name == "GM_FLAG_MAX_FLAGS")
        .expect("generated state-tag catalog omitted GM_FLAG_MAX_FLAGS")
        .code as u32;
    let valid_state_tag_flags = (1_u64 << state_tag_flag_count) - 1;
    if snapshot.trajectory.state_tag.packed_flags & !valid_state_tag_flags != 0 {
        issue(
            report,
            Severity::Error,
            category::UNKNOWN_CODE,
            "trajectory.state_tag_flags",
            STATE_TAG_FLAG.name,
            domain_id(STATE_TAG_FLAG),
            (snapshot.trajectory.state_tag.packed_flags & !valid_state_tag_flags) as i64,
            None,
            "state tag contains flag bits absent from LinuxCNC 2.9.10",
        );
        report.unknown_domain_mask |= 1_u64 << domain_id(STATE_TAG_FLAG);
    }
}

fn evaluate_joints(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    let joint_count = snapshot
        .trajectory
        .joints
        .clamp(0, EMCMOT_MAX_JOINTS as i32) as usize;
    for (index, joint) in snapshot.joints.iter().take(joint_count).enumerate() {
        check_rcs(
            report,
            &format!("joint[{index}]"),
            joint.rcs,
            CommandDomain::Motion,
            category::JOINT_RCS,
        );
        check_code(
            report,
            format!("joint[{index}].joint_type"),
            JOINT_TYPE,
            i64::from(joint.joint_type),
        );
        if joint.fault != 0 {
            issue(
                report,
                Severity::Error,
                category::JOINT_FAULT,
                format!("joint[{index}].fault"),
                "boolean_status",
                u32::MAX,
                i64::from(joint.fault),
                None,
                "LinuxCNC reports a joint amplifier/following fault",
            );
        }
        if joint.min_hard_limit != 0 || joint.max_hard_limit != 0 {
            issue(
                report,
                Severity::Warning,
                category::HARD_LIMIT,
                format!("joint[{index}].hard_limit"),
                "limit_status",
                u32::MAX,
                i64::from((joint.min_hard_limit != 0) as i32)
                    - i64::from((joint.max_hard_limit != 0) as i32),
                None,
                "joint hard-limit input is active",
            );
        }
        if joint.min_soft_limit != 0 || joint.max_soft_limit != 0 {
            issue(
                report,
                Severity::Warning,
                category::SOFT_LIMIT,
                format!("joint[{index}].soft_limit"),
                "limit_status",
                u32::MAX,
                i64::from((joint.min_soft_limit != 0) as i32)
                    - i64::from((joint.max_soft_limit != 0) as i32),
                None,
                "joint soft-limit status is active",
            );
        }
    }
}

fn evaluate_axes(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    for index in 0..EMCMOT_MAX_AXIS {
        if snapshot.trajectory.axis_mask & (1_i32 << index) == 0 {
            continue;
        }
        check_rcs(
            report,
            &format!("axis[{index}]"),
            snapshot.axes[index].rcs,
            CommandDomain::Motion,
            category::AXIS_RCS,
        );
    }
}

fn evaluate_spindles(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    let spindle_count = snapshot
        .trajectory
        .spindles
        .clamp(0, EMCMOT_MAX_SPINDLES as i32) as usize;
    for (index, spindle) in snapshot.spindles.iter().take(spindle_count).enumerate() {
        check_rcs(
            report,
            &format!("spindle[{index}]"),
            spindle.rcs,
            CommandDomain::Motion,
            category::SPINDLE_RCS,
        );
        check_i32_set(
            report,
            format!("spindle[{index}].direction"),
            spindle.direction,
            &[-1, 0, 1],
            "spindle direction is not reverse, stopped, or forward",
        );
        check_i32_set(
            report,
            format!("spindle[{index}].brake"),
            spindle.brake,
            &[0, 1],
            "spindle brake state is neither released nor engaged",
        );
        check_i32_set(
            report,
            format!("spindle[{index}].enabled"),
            spindle.enabled,
            &[0, 1],
            "spindle enabled state is neither false nor true",
        );
        let orient_name = check_code(
            report,
            format!("spindle[{index}].orient_state"),
            SPINDLE_ORIENT_STATE,
            i64::from(spindle.orient_state),
        );
        if orient_name == Some("EMCMOT_ORIENT_FAULTED") {
            issue(
                report,
                Severity::Error,
                category::SPINDLE_ORIENT,
                format!("spindle[{index}].orient_fault"),
                SPINDLE_ORIENT_STATE.name,
                domain_id(SPINDLE_ORIENT_STATE),
                i64::from(spindle.orient_fault),
                orient_name,
                "LinuxCNC spindle orientation reported a fault",
            );
        }
    }
}

fn evaluate_misc_errors(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    for (index, value) in snapshot.misc_error.iter().copied().enumerate() {
        if value != 0 {
            issue(
                report,
                Severity::Error,
                category::MISC_ERROR,
                format!("motion.misc_error[{index}]"),
                "misc_error",
                u32::MAX,
                i64::from(value),
                None,
                "LinuxCNC miscellaneous error input is active",
            );
        }
    }
}
