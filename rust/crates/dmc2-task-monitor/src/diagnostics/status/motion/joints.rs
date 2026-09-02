//! Every EMC_JOINT_STAT slot, including inactive structural slots.

use dmc2_linuxcnc_interface::{EMCMOT_MAX_JOINTS, JOINT_TYPE};

use crate::snapshot::NativeSnapshot;

use super::super::super::catalog::{check_code, issue};
use super::super::super::category;
use super::super::super::report::{DiagnosticReport, Severity};
use super::super::super::validation::{account_open, check_finite, check_u32_flag};
use super::super::rcs::{check_rcs, CommandDomain};

pub(super) fn evaluate(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    let configured = snapshot
        .trajectory
        .joints
        .clamp(0, EMCMOT_MAX_JOINTS as i32) as usize;
    for (index, joint) in snapshot.joints.iter().enumerate() {
        let prefix = format!("joints[{index}]");
        check_rcs(
            report,
            &format!("{prefix}.rcs"),
            "EMC_JOINT_STAT",
            joint.rcs,
            CommandDomain::Motion,
            category::JOINT_RCS,
        );
        account_open(
            report,
            format!("{prefix}.joint_number"),
            "open_linuxcnc_slot_identifier",
        );
        check_code(
            report,
            format!("{prefix}.joint_type"),
            JOINT_TYPE,
            i64::from(joint.joint_type),
        );
        for (field, value) in [
            ("units", joint.units),
            ("backlash", joint.backlash),
            ("min_position_limit", joint.min_position_limit),
            ("max_position_limit", joint.max_position_limit),
            ("max_ferror", joint.max_ferror),
            ("min_ferror", joint.min_ferror),
            ("ferror_current", joint.ferror_current),
            ("ferror_high_mark", joint.ferror_high_mark),
            ("output", joint.output),
            ("input", joint.input),
            ("velocity", joint.velocity),
        ] {
            check_finite(report, format!("{prefix}.{field}"), value);
        }
        for (field, value) in [
            ("in_position", joint.in_position),
            ("homing", joint.homing),
            ("homed", joint.homed),
            ("fault", joint.fault),
            ("enabled", joint.enabled),
            ("min_soft_limit", joint.min_soft_limit),
            ("max_soft_limit", joint.max_soft_limit),
            ("min_hard_limit", joint.min_hard_limit),
            ("max_hard_limit", joint.max_hard_limit),
            ("override_limits", joint.override_limits),
        ] {
            check_u32_flag(
                report,
                format!("{prefix}.{field}"),
                value,
                "joint status flag is neither false nor true",
            );
        }

        if index >= configured {
            continue;
        }
        if joint.fault == 1 {
            issue(
                report,
                Severity::Error,
                category::JOINT_FAULT,
                format!("{prefix}.fault"),
                "boolean_status",
                u32::MAX,
                i64::from(joint.fault),
                None,
                "LinuxCNC reports a joint amplifier/following fault",
            );
        }
        // This machine's positive home switch is also its positive hard-limit
        // input. LinuxCNC intentionally reports that input during both the
        // search and latch phases, and HOME_IGNORE_LIMITS authorizes exactly
        // that same-joint condition while `homing` is true. Retain hard-limit
        // diagnostics at every other time.
        if joint.homing != 1 && (joint.min_hard_limit == 1 || joint.max_hard_limit == 1) {
            issue(
                report,
                Severity::Warning,
                category::HARD_LIMIT,
                format!("{prefix}.hard_limit"),
                "limit_status",
                u32::MAX,
                i64::from((joint.min_hard_limit == 1) as i32)
                    - i64::from((joint.max_hard_limit == 1) as i32),
                None,
                "joint hard-limit input is active",
            );
        }
        if joint.min_soft_limit == 1 || joint.max_soft_limit == 1 {
            issue(
                report,
                Severity::Warning,
                category::SOFT_LIMIT,
                format!("{prefix}.soft_limit"),
                "limit_status",
                u32::MAX,
                i64::from((joint.min_soft_limit == 1) as i32)
                    - i64::from((joint.max_soft_limit == 1) as i32),
                None,
                "joint soft-limit status is active",
            );
        }
    }
}
