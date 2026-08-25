//! Source-backed classification of LinuxCNC status and error states.

mod catalog;
pub mod category;
mod report;
mod status;
mod transport;

use crate::snapshot::NativeSnapshot;

use catalog::{check_debug_mask, issue};
pub use report::{DiagnosticReport, Severity, TransitionLogger};
pub use transport::disconnected;

pub fn evaluate(snapshot: &NativeSnapshot) -> DiagnosticReport {
    let mut report = DiagnosticReport::default();
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
    check_debug_mask(&mut report, "top.debug", snapshot.top_debug);
    check_debug_mask(&mut report, "motion.debug", snapshot.motion_debug);
    check_debug_mask(&mut report, "io.debug", snapshot.io.debug);
    report
}

#[cfg(test)]
mod tests;
