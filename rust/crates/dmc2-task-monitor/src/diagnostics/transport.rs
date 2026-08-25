//! Status-channel transport failures.

use dmc2_linuxcnc_interface::NML_ERROR;

use super::catalog::{domain_id, issue, unknown_code};
use super::category;
use super::report::{DiagnosticReport, Severity};

pub fn disconnected(nml_error: i32) -> DiagnosticReport {
    let mut report = DiagnosticReport::default();
    let name = NML_ERROR.lookup(i64::from(nml_error));
    issue(
        &mut report,
        Severity::Error,
        category::TRANSPORT,
        "emcStatus",
        NML_ERROR.name,
        domain_id(NML_ERROR),
        i64::from(nml_error),
        name,
        "native LinuxCNC emcStatus channel is disconnected or unreadable",
    );
    if name.is_none() {
        unknown_code(
            &mut report,
            "emcStatus.error_type",
            NML_ERROR,
            i64::from(nml_error),
        );
    }
    report
}
