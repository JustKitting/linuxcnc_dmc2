//! Status-channel transport failures.

use dmc2_linuxcnc_interface::{CMS_STATUS, NML_ERROR};

use super::catalog::{domain_id, issue, unknown_code};
use super::category;
use super::report::{DiagnosticReport, Severity};

fn evaluate_nml(report: &mut DiagnosticReport, nml_error: i32, disconnected: bool) {
    let name = NML_ERROR.lookup(i64::from(nml_error));
    if disconnected || name != Some("NML_NO_ERROR") {
        issue(
            report,
            Severity::Error,
            category::TRANSPORT,
            "emcStatus.error_type",
            NML_ERROR.name,
            domain_id(NML_ERROR),
            i64::from(nml_error),
            name,
            if disconnected {
                "native LinuxCNC emcStatus channel is disconnected or unreadable"
            } else {
                "native LinuxCNC NML channel reported an error"
            },
        );
    }
    if name.is_none() {
        unknown_code(
            report,
            "emcStatus.error_type",
            NML_ERROR,
            i64::from(nml_error),
        );
    }
}

fn evaluate_cms(report: &mut DiagnosticReport, cms_status: i32, disconnected: bool) {
    let name = CMS_STATUS.lookup(i64::from(cms_status));
    if name.is_none() {
        unknown_code(
            report,
            "emcStatus.cms.status",
            CMS_STATUS,
            i64::from(cms_status),
        );
    } else if cms_status < 0 {
        issue(
            report,
            Severity::Error,
            category::TRANSPORT,
            "emcStatus.cms.status",
            CMS_STATUS.name,
            domain_id(CMS_STATUS),
            i64::from(cms_status),
            name,
            "native LinuxCNC CMS transport reported an error",
        );
    } else if !matches!(name, Some("CMS_READ_OLD" | "CMS_READ_OK"))
        && !(disconnected && name == Some("CMS_STATUS_NOT_SET"))
    {
        issue(
            report,
            Severity::Error,
            category::TRANSPORT,
            "emcStatus.cms.status",
            CMS_STATUS.name,
            domain_id(CMS_STATUS),
            i64::from(cms_status),
            name,
            "CMS state is impossible after the LinuxCNC emcStatus NML::peek operation",
        );
    }
}

pub(super) fn evaluate(report: &mut DiagnosticReport, nml_error: i32, cms_status: i32) {
    evaluate_nml(report, nml_error, false);
    evaluate_cms(report, cms_status, false);
}

pub fn disconnected(nml_error: i32, cms_status: i32) -> DiagnosticReport {
    let mut report = DiagnosticReport::default();
    evaluate_nml(&mut report, nml_error, true);
    evaluate_cms(&mut report, cms_status, true);
    report
}
