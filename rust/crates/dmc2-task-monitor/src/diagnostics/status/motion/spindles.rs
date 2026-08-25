//! Every EMC_SPINDLE_STAT slot and orientation-fault payload.

use dmc2_linuxcnc_interface::{EMCMOT_MAX_SPINDLES, SPINDLE_ORIENT_STATE};

use crate::snapshot::NativeSnapshot;

use super::super::super::catalog::{check_code, check_i32_set, issue};
use super::super::super::category;
use super::super::super::report::{DiagnosticReport, Severity};
use super::super::super::validation::{account_open, check_finite, check_u32_flag};
use super::super::rcs::{check_rcs, CommandDomain};

pub(super) fn evaluate(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    let configured = snapshot
        .trajectory
        .spindles
        .clamp(0, EMCMOT_MAX_SPINDLES as i32) as usize;
    for (index, spindle) in snapshot.spindles.iter().enumerate() {
        let prefix = format!("spindles[{index}]");
        check_rcs(
            report,
            &format!("{prefix}.rcs"),
            "EMC_SPINDLE_STAT",
            spindle.rcs,
            CommandDomain::Motion,
            category::SPINDLE_RCS,
        );
        for (field, value) in [
            ("speed", spindle.speed),
            ("spindle_scale", spindle.spindle_scale),
            ("css_maximum", spindle.css_maximum),
            ("css_factor", spindle.css_factor),
        ] {
            check_finite(report, format!("{prefix}.{field}"), value);
        }
        account_open(report, format!("{prefix}.state"), "open_spindle_state");
        for (field, value, detail) in [
            (
                "direction",
                spindle.direction,
                "spindle direction is not reverse, stopped, or forward",
            ),
            (
                "increasing",
                spindle.increasing,
                "spindle ramp state is not decreasing, constant, or increasing",
            ),
        ] {
            check_i32_set(
                report,
                format!("{prefix}.{field}"),
                value,
                &[-1, 0, 1],
                detail,
            );
        }
        check_i32_set(
            report,
            format!("{prefix}.brake"),
            spindle.brake,
            &[0, 1],
            "spindle brake state is neither released nor engaged",
        );
        check_i32_set(
            report,
            format!("{prefix}.enabled"),
            spindle.enabled,
            &[0, 1],
            "spindle enabled state is neither false nor true",
        );
        let orient_name = check_code(
            report,
            format!("{prefix}.orient_state"),
            SPINDLE_ORIENT_STATE,
            i64::from(spindle.orient_state),
        );
        account_open(
            report,
            format!("{prefix}.orient_fault"),
            "open_signed_hal_fault_payload",
        );
        check_u32_flag(
            report,
            format!("{prefix}.override_enabled"),
            spindle.override_enabled,
            "spindle-override state is neither false nor true",
        );
        check_u32_flag(
            report,
            format!("{prefix}.homed"),
            spindle.homed,
            "spindle homed state is neither false nor true",
        );
        if index < configured && orient_name == Some("EMCMOT_ORIENT_FAULTED") {
            issue(
                report,
                Severity::Error,
                category::SPINDLE_ORIENT,
                format!("{prefix}.orient_fault"),
                "spindle_orient_fault_payload",
                u32::MAX,
                i64::from(spindle.orient_fault),
                None,
                "LinuxCNC spindle orientation faulted; value is the open signed HAL fault payload",
            );
        }
    }
}
