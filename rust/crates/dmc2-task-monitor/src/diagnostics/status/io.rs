//! I/O, toolchanger, coolant, auxiliary, and lube status validation.

use crate::snapshot::NativeSnapshot;

use super::super::catalog::{check_i32_set, issue};
use super::super::category;
use super::super::report::{DiagnosticReport, Severity};
use super::rcs::{check_rcs, CommandDomain};

pub(super) fn evaluate(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    check_rcs(
        report,
        "io",
        snapshot.io.rcs,
        CommandDomain::EmcNml,
        category::IO_RCS,
    );
    for (source, status) in [
        ("io.tool", snapshot.io.tool.rcs),
        ("io.aux", snapshot.io.aux.rcs),
        ("io.coolant", snapshot.io.coolant.rcs),
        ("io.lube", snapshot.io.lube.rcs),
    ] {
        check_rcs(
            report,
            source,
            status,
            CommandDomain::EmcNml,
            category::IO_RCS,
        );
    }
    if snapshot.io.fault != 0 {
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
    for (source, value) in [
        ("io.estop", snapshot.io.aux.estop),
        ("io.coolant_mist", snapshot.io.coolant.mist),
        ("io.coolant_flood", snapshot.io.coolant.flood),
        ("io.lube_on", snapshot.io.lube.on),
        ("io.lube_level", snapshot.io.lube.level),
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
