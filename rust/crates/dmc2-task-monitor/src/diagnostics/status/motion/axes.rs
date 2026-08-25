//! Every EMC_AXIS_STAT slot and the derived stopped state.

use crate::snapshot::NativeSnapshot;

use super::super::super::category;
use super::super::super::report::DiagnosticReport;
use super::super::super::validation::{account_open, check_finite, check_u32_flag};
use super::super::rcs::{check_rcs, CommandDomain};

pub(super) fn evaluate(snapshot: &NativeSnapshot, report: &mut DiagnosticReport) {
    for (index, axis) in snapshot.axes.iter().enumerate() {
        let prefix = format!("axes[{index}]");
        check_rcs(
            report,
            &format!("{prefix}.rcs"),
            "EMC_AXIS_STAT",
            axis.rcs,
            CommandDomain::Motion,
            category::AXIS_RCS,
        );
        account_open(
            report,
            format!("{prefix}.axis_number"),
            "open_linuxcnc_slot_identifier",
        );
        check_finite(
            report,
            format!("{prefix}.min_position_limit"),
            axis.min_position_limit,
        );
        check_finite(
            report,
            format!("{prefix}.max_position_limit"),
            axis.max_position_limit,
        );
        check_finite(report, format!("{prefix}.velocity"), axis.velocity);
        check_u32_flag(
            report,
            format!("{prefix}.stopped"),
            axis.stopped,
            "derived axis-stopped state is neither false nor true",
        );
    }
}
