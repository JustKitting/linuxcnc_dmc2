//! EMC_MOTION_STAT fields outside trajectory/joint/axis/spindle aggregates.

use dmc2_linuxcnc_interface::EMCMOT_MAX_JOINTS;

use crate::snapshot::NativeSnapshot;

use super::super::super::catalog::{check_i32_set, issue};
use super::super::super::category;
use super::super::super::report::{DiagnosticReport, Severity};
use super::super::super::validation::{
    account_open, check_binary_i32_array, check_finite_f64_array, check_i32_range, check_pose,
    check_u32_flag,
};

pub(super) fn evaluate(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    account_open(report, "motion_heartbeat", "monotonic_heartbeat");
    check_binary_i32_array(
        report,
        "synchronized_digital_inputs",
        &snapshot.synchronized_digital_inputs,
    );
    check_binary_i32_array(
        report,
        "synchronized_digital_outputs",
        &snapshot.synchronized_digital_outputs,
    );
    check_finite_f64_array(report, "analog_inputs", &snapshot.analog_inputs);
    check_finite_f64_array(report, "analog_outputs", &snapshot.analog_outputs);
    check_binary_i32_array(report, "misc_error", &snapshot.misc_error);
    for (index, value) in snapshot.misc_error.iter().copied().enumerate() {
        if value == 1 {
            issue(
                report,
                Severity::Error,
                category::MISC_ERROR,
                format!("misc_error[{index}]"),
                "misc_error",
                u32::MAX,
                i64::from(value),
                None,
                "LinuxCNC miscellaneous error input is active",
            );
        }
    }
    check_i32_set(
        report,
        "on_soft_limit",
        snapshot.on_soft_limit,
        &[0, 1],
        "motion soft-limit state is neither false nor true",
    );
    if snapshot.on_soft_limit == 1 {
        issue(
            report,
            Severity::Warning,
            category::SOFT_LIMIT,
            "on_soft_limit",
            "boolean_status",
            u32::MAX,
            1,
            None,
            "LinuxCNC aggregate motion status reports an active soft limit",
        );
    }
    check_i32_set(
        report,
        "external_offsets_applied",
        snapshot.external_offsets_applied,
        &[0, 1],
        "external-offset-applied state is neither false nor true",
    );
    check_pose(
        report,
        "external_offset_pose",
        snapshot.external_offset_pose,
    );
    check_i32_range(
        report,
        "num_extra_joints",
        snapshot.num_extra_joints,
        0,
        EMCMOT_MAX_JOINTS as i32,
        "configured_count",
        "extra-joint count is outside LinuxCNC 2.9.10 bounds",
    );
    check_u32_flag(
        report,
        "jogging_active",
        snapshot.jogging_active,
        "aggregate jogging-active state is neither false nor true",
    );
}
