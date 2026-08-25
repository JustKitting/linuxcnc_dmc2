//! Structural validators shared by status-domain evaluators.

use crate::snapshot::PoseSnapshot;

use super::catalog::issue;
use super::category;
use super::report::{DiagnosticReport, Severity};

pub(super) fn account_open(
    report: &mut DiagnosticReport,
    source: impl Into<String>,
    policy: &'static str,
) {
    report.account(source, policy);
}

pub(super) fn check_u32_flag(
    report: &mut DiagnosticReport,
    source: impl Into<String>,
    value: u32,
    detail: &'static str,
) {
    let source = source.into();
    report.account(source.clone(), "boolean_flag");
    if value > 1 {
        issue(
            report,
            Severity::Error,
            category::INVALID_VALUE,
            source,
            "boolean_flag",
            u32::MAX,
            i64::from(value),
            None,
            detail,
        );
    }
}

pub(super) fn check_i32_range(
    report: &mut DiagnosticReport,
    source: impl Into<String>,
    value: i32,
    minimum: i32,
    maximum: i32,
    policy: &'static str,
    detail: &'static str,
) {
    let source = source.into();
    report.account(source.clone(), policy);
    if !(minimum..=maximum).contains(&value) {
        issue(
            report,
            Severity::Error,
            category::INVALID_VALUE,
            source,
            policy,
            u32::MAX,
            i64::from(value),
            None,
            detail,
        );
    }
}

pub(super) fn check_finite(report: &mut DiagnosticReport, source: impl Into<String>, value: f64) {
    let source = source.into();
    report.account(source.clone(), "finite_f64");
    if !value.is_finite() {
        issue(
            report,
            Severity::Error,
            category::INVALID_VALUE,
            source,
            "finite_f64",
            u32::MAX,
            value.to_bits() as i64,
            None,
            "LinuxCNC status contains a non-finite floating-point value",
        );
    }
}

pub(super) fn check_finite_f32_array(
    report: &mut DiagnosticReport,
    source: impl Into<String>,
    values: &[f32],
) {
    let source = source.into();
    report.account(source.clone(), "finite_f32_array");
    for (index, value) in values.iter().copied().enumerate() {
        if !value.is_finite() {
            issue(
                report,
                Severity::Error,
                category::INVALID_VALUE,
                format!("{source}[{index}]"),
                "finite_f32",
                u32::MAX,
                i64::from(value.to_bits()),
                None,
                "LinuxCNC state-tag array contains a non-finite floating-point value",
            );
        }
    }
}

pub(super) fn check_finite_f64_array(
    report: &mut DiagnosticReport,
    source: impl Into<String>,
    values: &[f64],
) {
    let source = source.into();
    report.account(source.clone(), "finite_f64_array");
    for (index, value) in values.iter().copied().enumerate() {
        if !value.is_finite() {
            issue(
                report,
                Severity::Error,
                category::INVALID_VALUE,
                format!("{source}[{index}]"),
                "finite_f64",
                u32::MAX,
                value.to_bits() as i64,
                None,
                "LinuxCNC status array contains a non-finite floating-point value",
            );
        }
    }
}

pub(super) fn check_binary_i32_array(
    report: &mut DiagnosticReport,
    source: impl Into<String>,
    values: &[i32],
) {
    let source = source.into();
    report.account(source.clone(), "boolean_i32_array");
    for (index, value) in values.iter().copied().enumerate() {
        if !matches!(value, 0 | 1) {
            issue(
                report,
                Severity::Error,
                category::INVALID_VALUE,
                format!("{source}[{index}]"),
                "boolean_integer",
                u32::MAX,
                i64::from(value),
                None,
                "LinuxCNC status array contains a value other than zero or one",
            );
        }
    }
}

pub(super) fn check_bounded_c_bytes(
    report: &mut DiagnosticReport,
    source: impl Into<String>,
    bytes: &[u8],
) {
    let source = source.into();
    report.account(source.clone(), "bounded_c_bytes");
    if !bytes.contains(&0) {
        issue(
            report,
            Severity::Error,
            category::INVALID_VALUE,
            source,
            "bounded_c_bytes",
            u32::MAX,
            bytes.len().try_into().unwrap_or(i64::MAX),
            None,
            "fixed LinuxCNC character array has no in-bounds NUL terminator",
        );
    }
}

pub(super) fn check_pose(report: &mut DiagnosticReport, source: &str, pose: PoseSnapshot) {
    for (axis, value) in [
        ("x", pose.x),
        ("y", pose.y),
        ("z", pose.z),
        ("a", pose.a),
        ("b", pose.b),
        ("c", pose.c),
        ("u", pose.u),
        ("v", pose.v),
        ("w", pose.w),
    ] {
        check_finite(report, format!("{source}.{axis}"), value);
    }
}
