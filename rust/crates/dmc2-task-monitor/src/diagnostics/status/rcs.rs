//! Common validation for every nested LinuxCNC RCS status message.

use dmc2_linuxcnc_interface::{
    CodeDomain, EMC_NML_MESSAGE_TYPE, MOTION_COMMAND, RCS_GENERIC_COMMAND, RCS_STATE, RCS_STATUS,
};

use crate::snapshot::RcsStatusSnapshot;

use super::super::catalog::{check_code, domain_id, issue, unknown_code};
use super::super::category;
use super::super::report::{DiagnosticReport, Severity};

#[derive(Clone, Copy)]
pub(super) enum CommandDomain {
    EmcNml,
    Motion,
}

impl CommandDomain {
    const fn catalog(self) -> CodeDomain {
        match self {
            Self::EmcNml => EMC_NML_MESSAGE_TYPE,
            Self::Motion => MOTION_COMMAND,
        }
    }
}

pub(super) fn check_rcs(
    report: &mut DiagnosticReport,
    source: &str,
    status: RcsStatusSnapshot,
    commands: CommandDomain,
    fault_category: u64,
) {
    let command_type = i64::from(status.command_type);
    if status.command_type != -1
        && status.command_type != 0
        && commands.catalog().lookup(command_type).is_none()
        && RCS_GENERIC_COMMAND.lookup(command_type).is_none()
    {
        unknown_code(
            report,
            format!("{source}.command_type"),
            commands.catalog(),
            command_type,
        );
    }

    let rcs_name = check_code(
        report,
        format!("{source}.status"),
        RCS_STATUS,
        i64::from(status.status),
    );
    check_code(
        report,
        format!("{source}.state"),
        RCS_STATE,
        i64::from(status.state),
    );

    if rcs_name == Some("RCS_ERROR") {
        issue(
            report,
            Severity::Error,
            fault_category,
            format!("{source}.status"),
            RCS_STATUS.name,
            domain_id(RCS_STATUS),
            i64::from(status.status),
            rcs_name,
            "LinuxCNC subsystem reported RCS_ERROR",
        );
    }
    if status.reserved != 0 {
        issue(
            report,
            Severity::Error,
            category::ABI,
            format!("{source}.reserved"),
            "snapshot_abi",
            u32::MAX,
            i64::from(status.reserved),
            None,
            "reserved ABI field was modified",
        );
    }
}
