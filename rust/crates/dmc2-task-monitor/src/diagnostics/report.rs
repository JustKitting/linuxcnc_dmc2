//! Diagnostic result model and edge-triggered logging.

use std::collections::BTreeSet;

#[cfg(test)]
use std::collections::BTreeMap;

use super::category;

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
    pub severity: Severity,
    pub category: u64,
    pub source: String,
    pub domain: &'static str,
    pub domain_id: u32,
    pub value: i64,
    pub name: Option<&'static str>,
    pub detail: &'static str,
}

#[derive(Clone, Debug, Default)]
pub struct DiagnosticReport {
    pub active_error_mask: u64,
    pub active_warning_mask: u64,
    pub unknown_domain_mask: u64,
    pub issues: Vec<Issue>,
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

    pub(super) fn push(&mut self, issue: Issue) {
        match issue.severity {
            Severity::Warning => self.active_warning_mask |= issue.category,
            Severity::Error => self.active_error_mask |= issue.category,
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

    #[cfg(test)]
    pub(super) fn covered_fields(&self) -> &BTreeMap<String, &'static str> {
        &self.covered_fields
    }
}

#[derive(Default)]
pub struct TransitionLogger {
    active: BTreeSet<Issue>,
}

#[derive(Clone, Debug, Default)]
pub struct TransitionUpdate {
    pub count: u32,
    pub latest: Option<Issue>,
    pub latest_action: i32,
}

impl TransitionLogger {
    pub fn update(&mut self, report: &DiagnosticReport) -> TransitionUpdate {
        let next = report.issues.iter().cloned().collect::<BTreeSet<_>>();
        let mut update = TransitionUpdate::default();
        for issue in next.difference(&self.active) {
            log_issue("assert", issue);
            update.count = update.count.saturating_add(1);
            update.latest = Some(issue.clone());
            update.latest_action = 1;
        }
        for issue in self.active.difference(&next) {
            log_issue("clear", issue);
            update.count = update.count.saturating_add(1);
            update.latest = Some(issue.clone());
            update.latest_action = -1;
        }
        self.active = next;
        update
    }
}

fn log_issue(action: &str, issue: &Issue) {
    eprintln!(
        "DMC2_LINUXCNC_DIAGNOSTIC action={} severity={} category=0x{:016x} source={} domain={} domain_id={} code={} name={} detail={:?}",
        action,
        issue.severity.as_str(),
        issue.category,
        issue.source,
        issue.domain,
        issue.domain_id,
        issue.value,
        issue.name.unwrap_or("UNKNOWN"),
        issue.detail,
    );
}
