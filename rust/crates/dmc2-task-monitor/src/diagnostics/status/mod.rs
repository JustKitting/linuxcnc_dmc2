//! Validation of the complete native LinuxCNC status snapshot.

mod io;
mod motion;
mod rcs;
mod task;

use crate::snapshot::NativeSnapshot;

use super::category;
use super::report::DiagnosticReport;
use rcs::{check_rcs, CommandDomain};

pub(super) fn evaluate(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    check_rcs(
        report,
        "top",
        snapshot.top_rcs,
        CommandDomain::EmcNml,
        category::TOP_RCS,
    );
    check_rcs(
        report,
        "task",
        snapshot.task.rcs,
        CommandDomain::EmcNml,
        category::TASK_RCS,
    );
    check_rcs(
        report,
        "motion",
        snapshot.motion_rcs,
        CommandDomain::Motion,
        category::MOTION_RCS,
    );
    check_rcs(
        report,
        "trajectory",
        snapshot.trajectory.rcs,
        CommandDomain::Motion,
        category::TRAJECTORY_RCS,
    );

    task::evaluate(snapshot, report);
    motion::evaluate(snapshot, report);
    io::evaluate(snapshot, report);
}
