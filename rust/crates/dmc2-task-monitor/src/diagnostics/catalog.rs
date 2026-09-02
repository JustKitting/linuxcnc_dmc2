//! Version-locked LinuxCNC code lookup and primitive value validators.

use dmc2_linuxcnc_interface::{CodeDomain, DEBUG_FLAG, DOMAINS};

use super::category;
use super::report::{DiagnosticReport, Issue, Severity};

pub(super) fn domain_id(domain: CodeDomain) -> u32 {
    DOMAINS
        .iter()
        .position(|candidate| candidate.name == domain.name)
        .expect("generated LinuxCNC domain was omitted from DOMAINS")
        .try_into()
        .expect("LinuxCNC domain index does not fit in u32")
}

// Every argument maps directly to one field of `Issue`; keeping that mapping
// visible at call sites prevents diagnostic provenance from being defaulted.
#[allow(clippy::too_many_arguments)]
pub(super) fn issue(
    report: &mut DiagnosticReport,
    severity: Severity,
    category: u64,
    source: impl Into<String>,
    domain: &'static str,
    domain_id: u32,
    value: i64,
    name: Option<&'static str>,
    detail: &'static str,
) {
    let source = source.into();
    let evidence = format!("source={source:?} raw={value}");
    issue_with_evidence(
        report, severity, category, source, domain, domain_id, value, name, detail, evidence,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn issue_with_evidence(
    report: &mut DiagnosticReport,
    severity: Severity,
    category: u64,
    source: impl Into<String>,
    domain: &'static str,
    domain_id: u32,
    value: i64,
    name: Option<&'static str>,
    detail: &'static str,
    evidence: impl Into<String>,
) {
    report.push(Issue::new(
        severity,
        category,
        source,
        domain,
        domain_id,
        value,
        name,
        detail,
        operator_action(category),
        evidence,
    ));
}

fn operator_action(category_value: u64) -> &'static str {
    match category_value {
        category::ABI => {
            "stop this monitor and rebuild/reinstall it against the pinned LinuxCNC 2.9.10 interface"
        }
        category::TRANSPORT => {
            "restore the LinuxCNC status-channel connection and confirm fresh valid status before resuming"
        }
        category::UNKNOWN_CODE => {
            "retain the raw domain/value, do not guess its meaning, and verify the running LinuxCNC source/version"
        }
        category::HARD_LIMIT => {
            "inspect the named joint/axis and use only the configured limit recovery path after the physical cause is clear"
        }
        category::SOFT_LIMIT => {
            "inspect the named axis position and commanded path, then correct the program or coordinate state"
        }
        category::INPUT_TIMEOUT => {
            "inspect the named input and its configured timeout source before retrying"
        }
        category::INTERPRETER => {
            "inspect the named interpreter result and correct the loaded program at its reported context"
        }
        category::IO_FAULT | category::JOINT_FAULT | category::SPINDLE_ORIENT => {
            "inspect the named source and retained LinuxCNC state, clear the physical/controller cause, then reset"
        }
        category::INVALID_VALUE => {
            "inspect the named source and correct it to one value from its documented domain"
        }
        category::STATUS_MESSAGE => {
            "read the exact LinuxCNC status message and correct its named source before continuing"
        }
        _ => {
            "inspect the named LinuxCNC source, raw value, and cause; clear the underlying condition before continuing"
        }
    }
}

pub(super) fn unknown_code(
    report: &mut DiagnosticReport,
    source: impl Into<String>,
    domain: CodeDomain,
    value: i64,
) {
    issue(
        report,
        Severity::Error,
        category::UNKNOWN_CODE,
        source,
        domain.name,
        domain_id(domain),
        value,
        None,
        "value is absent from the version-locked LinuxCNC 2.9.10 source catalog",
    );
}

pub(super) fn check_code(
    report: &mut DiagnosticReport,
    source: impl Into<String>,
    domain: CodeDomain,
    value: i64,
) -> Option<&'static str> {
    let source = source.into();
    report.account(source.clone(), "source_enum");
    match domain.lookup(value) {
        Some(name) => Some(name),
        None => {
            unknown_code(report, source, domain, value);
            None
        }
    }
}

pub(super) fn check_code_with_sentinels(
    report: &mut DiagnosticReport,
    source: impl Into<String>,
    domain: CodeDomain,
    value: i64,
    sentinels: &[i64],
) -> Option<&'static str> {
    let source = source.into();
    report.account(source.clone(), "source_enum_with_sentinel");
    if sentinels.contains(&value) {
        return None;
    }
    match domain.lookup(value) {
        Some(name) => Some(name),
        None => {
            unknown_code(report, source, domain, value);
            None
        }
    }
}

pub(super) fn check_i32_set(
    report: &mut DiagnosticReport,
    source: impl Into<String>,
    value: i32,
    allowed: &[i32],
    detail: &'static str,
) {
    let source = source.into();
    report.account(source.clone(), "constrained_integer");
    if !allowed.contains(&value) {
        issue(
            report,
            Severity::Error,
            category::INVALID_VALUE,
            source,
            "constrained_integer",
            u32::MAX,
            i64::from(value),
            None,
            detail,
        );
    }
}

pub(super) fn check_debug_mask(report: &mut DiagnosticReport, source: &str, raw: i32) {
    report.account(source, "source_bitmask");
    let allowed = DEBUG_FLAG
        .codes
        .iter()
        .fold(0_u32, |mask, code| mask | code.code as u32);
    let unknown = (raw as u32) & !allowed;
    if unknown != 0 {
        issue(
            report,
            Severity::Error,
            category::UNKNOWN_CODE,
            source,
            DEBUG_FLAG.name,
            domain_id(DEBUG_FLAG),
            i64::from(unknown),
            None,
            "debug mask contains bits absent from LinuxCNC 2.9.10",
        );
        report.unknown_domain_mask |= 1_u64 << domain_id(DEBUG_FLAG);
    }
}
