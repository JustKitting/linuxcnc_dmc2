//! Diagnostic result model and edge-triggered logging.

use std::collections::BTreeMap;

use super::category::{self, DiagnosticCategory};
use dmc2_diagnostics::{
    diagnostic_catalog, valid_diagnostic_domain, valid_symbolic_identity, RecoveryClass,
    RecoveryClassified,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Severity {
    Warning,
    Error,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Issue {
    severity: Severity,
    category: DiagnosticCategory,
    source: String,
    domain: &'static str,
    domain_id: u32,
    value: i64,
    name: Option<&'static str>,
    detail: &'static str,
    operator_action: &'static str,
    recovery_class: RecoveryClass,
    /// Complete captured context for this exact assertion. This is persisted
    /// with the identity/cause/action so an operator never has to reconstruct
    /// a numeric failure from a separate pin lookup.
    evidence: String,
    // Prevent construction outside this module. Every issue must pass the
    // complete identity/cause/action/evidence contract in `Issue::new`.
    _validated: (),
}

impl Issue {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new<R: RecoveryClassified + ?Sized>(
        severity: Severity,
        category: DiagnosticCategory,
        source: impl Into<String>,
        domain: &'static str,
        domain_id: u32,
        value: i64,
        name: Option<&'static str>,
        detail: &'static str,
        operator_action: &'static str,
        recovery_source: &R,
        evidence: impl Into<String>,
    ) -> Self {
        let source = source.into();
        let evidence = evidence.into();
        let recovery_class = recovery_source.recovery_class();
        let invalid_reason = if category.mask() == 0 {
            Some("zero diagnostic category")
        } else if source.is_empty() {
            Some("empty diagnostic source")
        } else if !valid_diagnostic_domain(domain) {
            Some("invalid diagnostic domain")
        } else if detail.is_empty() {
            Some("empty diagnostic cause")
        } else if operator_action.is_empty() {
            Some("empty diagnostic operator action")
        } else if evidence.is_empty() {
            Some("empty diagnostic evidence")
        } else if !recovery_class.contract_complete() {
            Some("incomplete recovery-class contract")
        } else if name.is_some_and(|identity| !valid_symbolic_identity(identity)) {
            Some("invalid symbolic diagnostic identity")
        } else {
            None
        };
        if let Some(reason) = invalid_reason {
            return Self {
                severity: Severity::Error,
                category: category::DIAGNOSTIC_INTERFACE,
                source: "dmc2-task-monitor.diagnostic-construction".to_owned(),
                domain: "dmc2_diagnostic_contract",
                domain_id: u32::MAX,
                value: 0,
                name: Some("DIAGNOSTIC_CONTRACT_INVALID"),
                detail: "a diagnostic producer violated the typed identity, cause, action, evidence, or recovery contract",
                operator_action: "retain the original fields below, correct the named diagnostic producer, and relaunch DMC2 LinuxCNC",
                recovery_class: RecoveryClass::RelaunchApplication,
                evidence: format!(
                    "reason={reason:?} original_category=0x{:016x} original_source={source:?} original_domain={domain:?} original_domain_id={domain_id} original_value={value} original_name={name:?} original_detail={detail:?} original_action={operator_action:?} original_recovery_class={} original_evidence={evidence:?}",
                    category.mask(),
                    recovery_class.name(),
                ),
                _validated: (),
            };
        }
        Self {
            severity,
            category,
            source,
            domain,
            domain_id,
            value,
            name,
            detail,
            operator_action,
            recovery_class,
            evidence,
            _validated: (),
        }
    }

    pub(crate) const fn severity(&self) -> Severity {
        self.severity
    }

    pub(crate) const fn category(&self) -> DiagnosticCategory {
        self.category
    }

    pub(crate) const fn recovery_class(&self) -> RecoveryClass {
        self.recovery_class
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    pub(crate) const fn domain(&self) -> &'static str {
        self.domain
    }

    pub(crate) const fn domain_id(&self) -> u32 {
        self.domain_id
    }

    pub(crate) const fn value(&self) -> i64 {
        self.value
    }

    pub(crate) const fn name(&self) -> Option<&'static str> {
        self.name
    }

    pub(crate) const fn detail(&self) -> &'static str {
        self.detail
    }

    pub(crate) const fn operator_action(&self) -> &'static str {
        self.operator_action
    }

    pub(crate) fn evidence(&self) -> &str {
        &self.evidence
    }
}

#[derive(Clone, Debug, Default)]
pub struct DiagnosticReport {
    pub active_error_mask: u64,
    pub active_warning_mask: u64,
    pub unknown_domain_mask: u64,
    issues: Vec<Issue>,
    #[cfg(test)]
    covered_fields: BTreeMap<String, &'static str>,
}

impl PartialEq for DiagnosticReport {
    fn eq(&self, other: &Self) -> bool {
        self.active_error_mask == other.active_error_mask
            && self.active_warning_mask == other.active_warning_mask
            && self.unknown_domain_mask == other.unknown_domain_mask
            && self.issues == other.issues
    }
}

impl Eq for DiagnosticReport {}

impl DiagnosticReport {
    pub fn error_active(&self) -> bool {
        self.active_error_mask != 0
    }

    pub fn warning_active(&self) -> bool {
        self.active_warning_mask != 0
    }

    pub fn unknown_code_active(&self) -> bool {
        self.unknown_domain_mask != 0
    }

    pub fn unknown_code_count(&self) -> u32 {
        self.issues
            .iter()
            .filter(|issue| issue.category == category::UNKNOWN_CODE)
            .count()
            .try_into()
            .unwrap_or(u32::MAX)
    }

    pub(crate) fn issue_count(&self) -> usize {
        self.issues.len()
    }

    pub(super) fn push(&mut self, issue: Issue) {
        match issue.severity {
            Severity::Warning => self.active_warning_mask |= issue.category.mask(),
            Severity::Error => self.active_error_mask |= issue.category.mask(),
        }
        if issue.category == category::UNKNOWN_CODE && issue.domain_id < 64 {
            self.unknown_domain_mask |= 1_u64 << issue.domain_id;
        }
        self.issues.push(issue);
    }

    #[cfg(test)]
    pub(super) fn account(&mut self, source: impl Into<String>, policy: &'static str) {
        let source = source.into();
        assert!(
            self.covered_fields.insert(source.clone(), policy).is_none(),
            "snapshot field {source} was assigned more than one diagnostic policy"
        );
    }

    #[cfg(not(test))]
    pub(super) fn account(&mut self, _source: impl Into<String>, _policy: &'static str) {}
}

#[derive(Default)]
pub struct TransitionLogger {
    active: BTreeMap<IssueIdentity, Issue>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct IssueIdentity {
    severity: Severity,
    category: DiagnosticCategory,
    source: String,
    domain: &'static str,
    domain_id: u32,
    value: i64,
    name: Option<&'static str>,
    detail: &'static str,
    operator_action: &'static str,
    recovery_class: RecoveryClass,
}

impl From<&Issue> for IssueIdentity {
    fn from(issue: &Issue) -> Self {
        Self {
            severity: issue.severity,
            category: issue.category,
            source: issue.source.clone(),
            domain: issue.domain,
            domain_id: issue.domain_id,
            value: issue.value,
            name: issue.name,
            detail: issue.detail,
            operator_action: issue.operator_action,
            recovery_class: issue.recovery_class,
        }
    }
}

diagnostic_catalog! {
    pub enum TransitionAction: i32 {
        Clear = -1 => (
            "CLEAR",
            "clear",
            "the previously active diagnostic condition is no longer present",
            "retain the transition history; no action is required solely because it cleared"
        ),
        Assert = 1 => (
            "ASSERT",
            "assert",
            "the diagnostic condition became active",
            "follow the diagnostic's specific operator action"
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticTransition {
    pub action: TransitionAction,
    pub issue: Issue,
}

impl DiagnosticTransition {
    pub fn identity(&self) -> String {
        self.issue.name.map(str::to_owned).unwrap_or_else(|| {
            format!(
                "UNKNOWN_{}(raw={})",
                self.issue.domain.to_ascii_uppercase(),
                self.issue.value
            )
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct TransitionUpdate {
    pub count: u32,
    pub latest: Option<Issue>,
    pub latest_action: i32,
    pub events: Vec<DiagnosticTransition>,
}

impl TransitionLogger {
    pub fn update(&mut self, report: &DiagnosticReport) -> TransitionUpdate {
        let mut next = BTreeMap::new();
        let mut update = TransitionUpdate::default();
        for issue in &report.issues {
            let identity = IssueIdentity::from(issue);
            if next.contains_key(&identity) {
                continue;
            }
            if let Some(active) = self.active.get(&identity) {
                next.insert(identity, active.clone());
                continue;
            }
            let event = DiagnosticTransition {
                action: TransitionAction::Assert,
                issue: issue.clone(),
            };
            log_issue(&event);
            update.count = update.count.saturating_add(1);
            update.latest = Some(issue.clone());
            update.latest_action = event.action.wire_code();
            update.events.push(event);
            next.insert(identity, issue.clone());
        }
        for (identity, issue) in &self.active {
            if next.contains_key(identity) {
                continue;
            }
            let event = DiagnosticTransition {
                action: TransitionAction::Clear,
                issue: issue.clone(),
            };
            log_issue(&event);
            update.count = update.count.saturating_add(1);
            update.latest = Some(issue.clone());
            update.latest_action = event.action.wire_code();
            update.events.push(event);
        }
        self.active = next;
        update
    }
}

fn log_issue(event: &DiagnosticTransition) {
    let issue = &event.issue;
    eprintln!(
        "DMC2_LINUXCNC_DIAGNOSTIC transition={} severity={} category=0x{:016x} source={} domain={} domain_id={} code={} identity={:?} cause={:?} operator_action={:?} recovery_class={} recovery_transition={} clear_condition={:?} ui_operations={:?} evidence={:?}",
        event.action.hal_slug(),
        issue.severity.as_str(),
        issue.category.mask(),
        issue.source,
        issue.domain,
        issue.domain_id,
        issue.value,
        event.identity(),
        issue.detail,
        issue.operator_action,
        issue.recovery_class.name(),
        issue.recovery_class.transition().name(),
        issue.recovery_class.clear_transition(),
        issue.recovery_class.ui_operations(),
        issue.evidence,
    );
}
