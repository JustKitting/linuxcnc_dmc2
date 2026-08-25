//! Source-backed classification of LinuxCNC status and error states.

mod catalog;
pub mod category;
mod report;
mod status;
mod transport;
mod validation;

use crate::snapshot::NativeSnapshot;

use catalog::{check_debug_mask, issue};
pub use report::{DiagnosticReport, Severity, TransitionLogger};
pub use transport::disconnected;

pub fn evaluate(snapshot: &NativeSnapshot) -> DiagnosticReport {
    let mut report = DiagnosticReport::default();
    report.account("abi_version", "exact_snapshot_abi");
    report.account("struct_size", "exact_snapshot_size");
    if !snapshot.valid_abi() {
        issue(
            &mut report,
            Severity::Error,
            category::ABI,
            "snapshot",
            "snapshot_abi",
            u32::MAX,
            i64::from(snapshot.abi_version),
            None,
            "native LinuxCNC status snapshot ABI does not match Rust",
        );
        return report;
    }

    status::evaluate(snapshot, &mut report);
    check_debug_mask(&mut report, "top_debug", snapshot.top_debug);
    check_debug_mask(&mut report, "motion_debug", snapshot.motion_debug);
    check_debug_mask(&mut report, "io.debug", snapshot.io.debug);
    report
}

pub fn evaluate_with_transport(
    snapshot: &NativeSnapshot,
    nml_error: i32,
    cms_status: i32,
) -> DiagnosticReport {
    let mut report = evaluate(snapshot);
    transport::evaluate(&mut report, nml_error, cms_status);
    report
}

#[cfg(test)]
mod tests;
