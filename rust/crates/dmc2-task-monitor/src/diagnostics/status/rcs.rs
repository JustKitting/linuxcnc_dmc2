//! Common validation for every nested LinuxCNC RCS status message.

use dmc2_linuxcnc_interface::{
    status_message_contract, CodeDomain, EMC_NML_MESSAGE_TYPE, MOTION_COMMAND, RCS_GENERIC_COMMAND,
    RCS_STATE, RCS_STATUS,
};

use crate::snapshot::RcsStatusSnapshot;

use super::super::catalog::{check_code, domain_id, issue, unknown_code};
use super::super::category;
use super::super::report::{DiagnosticReport, Severity};
use super::super::validation::{account_open, check_bounded_c_bytes};

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
    contract_class: &'static str,
    status: RcsStatusSnapshot,
    commands: CommandDomain,
    fault_category: u64,
) {
    let contract = status_message_contract(contract_class)
        .unwrap_or_else(|| panic!("generated LinuxCNC catalog omitted {contract_class}"));
    let message_type_name = check_code(
        report,
        format!("{source}.message_type"),
        EMC_NML_MESSAGE_TYPE,
        i64::from(status.message_type),
    );
    if i64::from(status.message_type) != contract.message_type {
        issue(
            report,
            Severity::Error,
            category::STATUS_MESSAGE,
            format!("{source}.message_type"),
            EMC_NML_MESSAGE_TYPE.name,
            domain_id(EMC_NML_MESSAGE_TYPE),
            i64::from(status.message_type),
            message_type_name,
            "RCS status object has the wrong LinuxCNC message type for its source field",
        );
    }
    report.account(
        format!("{source}.message_size"),
        "exact_status_message_size",
    );
    if status.message_size != contract.message_size {
        issue(
            report,
            Severity::Error,
            category::STATUS_MESSAGE,
            format!("{source}.message_size"),
            "status_message_size",
            u32::MAX,
            status.message_size,
            None,
            "RCS status object size differs from the exact LinuxCNC 2.9.10 C++ class size",
        );
    }

    let command_type = i64::from(status.command_type);
    report.account(format!("{source}.command_type"), "source_command_echo");
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
    account_open(
        report,
        format!("{source}.echo_serial_number"),
        "open_command_serial",
    );
    account_open(report, format!("{source}.line"), "open_line_number");
    account_open(report, format!("{source}.source_line"), "open_line_number");
    check_bounded_c_bytes(report, format!("{source}.source_file"), &status.source_file);
    report.account(format!("{source}.reserved"), "reserved_zero");

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
