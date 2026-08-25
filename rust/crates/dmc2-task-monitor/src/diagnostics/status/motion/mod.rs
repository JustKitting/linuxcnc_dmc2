//! Motion status, split by LinuxCNC status aggregate.

mod aggregate;
mod axes;
mod joints;
mod spindles;
mod trajectory;

use crate::snapshot::NativeSnapshot;

use super::super::report::DiagnosticReport;

pub(super) fn evaluate(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    trajectory::evaluate(snapshot, report);
    joints::evaluate(snapshot, report);
    axes::evaluate(snapshot, report);
    spindles::evaluate(snapshot, report);
    aggregate::evaluate(snapshot, report);
}
