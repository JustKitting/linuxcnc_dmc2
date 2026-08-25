//! I/O, toolchanger, coolant, auxiliary, and lube status validation.

use crate::snapshot::NativeSnapshot;

use super::super::catalog::{check_i32_set, issue};
use super::super::category;
use super::super::report::{DiagnosticReport, Severity};
use super::super::validation::{account_open, check_finite, check_pose};
use super::rcs::{check_rcs, CommandDomain};

pub(super) fn evaluate(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    check_rcs(
        report,
        "io.rcs",
        "EMC_IO_STAT",
        snapshot.io.rcs,
        CommandDomain::EmcNml,
        category::IO_RCS,
    );
    for (source, contract_class, status) in [
        ("io.tool.rcs", "EMC_TOOL_STAT", snapshot.io.tool.rcs),
        ("io.aux.rcs", "EMC_AUX_STAT", snapshot.io.aux.rcs),
        (
            "io.coolant.rcs",
            "EMC_COOLANT_STAT",
            snapshot.io.coolant.rcs,
        ),
        ("io.lube.rcs", "EMC_LUBE_STAT", snapshot.io.lube.rcs),
    ] {
        check_rcs(
            report,
            source,
            contract_class,
            status,
            CommandDomain::EmcNml,
            category::IO_RCS,
        );
    }
    account_open(report, "io.heartbeat", "monotonic_heartbeat");
    check_finite(report, "io.cycle_time", snapshot.io.cycle_time);
    account_open(
        report,
        "io.reason",
        "open_signed_toolchanger_reason_payload",
    );
    check_i32_set(
        report,
        "io.fault",
        snapshot.io.fault,
        &[0, 1],
        "toolchanger fault state is neither false nor true",
    );
    if snapshot.io.fault == 1 {
        issue(
            report,
            if snapshot.io.reason > 0 {
                Severity::Warning
            } else {
                Severity::Error
            },
            category::IO_FAULT,
            "io.fault",
            "io_fault_payload",
            u32::MAX,
            i64::from(snapshot.io.reason),
            None,
            "LinuxCNC IO/toolchanger fault is active; value is its reason payload",
        );
    }

    for (source, _) in [
        ("io.tool.pocket_prepped", snapshot.io.tool.pocket_prepped),
        ("io.tool.tool_in_spindle", snapshot.io.tool.tool_in_spindle),
        (
            "io.tool.tool_from_pocket",
            snapshot.io.tool.tool_from_pocket,
        ),
        (
            "io.tool.current_tool.tool_number",
            snapshot.io.tool.current_tool.tool_number,
        ),
        (
            "io.tool.current_tool.pocket_number",
            snapshot.io.tool.current_tool.pocket_number,
        ),
        (
            "io.tool.current_tool.orientation",
            snapshot.io.tool.current_tool.orientation,
        ),
    ] {
        account_open(report, source, "open_tool_table_integer");
    }
    check_pose(
        report,
        "io.tool.current_tool.offset",
        snapshot.io.tool.current_tool.offset,
    );
    for (source, value) in [
        (
            "io.tool.current_tool.diameter",
            snapshot.io.tool.current_tool.diameter,
        ),
        (
            "io.tool.current_tool.front_angle",
            snapshot.io.tool.current_tool.front_angle,
        ),
        (
            "io.tool.current_tool.back_angle",
            snapshot.io.tool.current_tool.back_angle,
        ),
    ] {
        check_finite(report, source, value);
    }
    for (source, value) in [
        ("io.aux.estop", snapshot.io.aux.estop),
        ("io.coolant.mist", snapshot.io.coolant.mist),
        ("io.coolant.flood", snapshot.io.coolant.flood),
        ("io.lube.on", snapshot.io.lube.on),
        ("io.lube.level", snapshot.io.lube.level),
    ] {
        check_i32_set(
            report,
            source,
            value,
            &[0, 1],
            "LinuxCNC boolean status is neither false nor true",
        );
    }
}
