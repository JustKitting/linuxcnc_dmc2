use crate::diagnostics::{DiagnosticReport, Severity, TransitionLogger};

#[derive(Default)]
pub(super) struct DiagnosticState {
    logger: TransitionLogger,
    pub(super) latched_error_mask: u64,
    pub(super) latched_warning_mask: u64,
    pub(super) transitions: u32,
    pub(super) latest_code_domain: i32,
    pub(super) latest_code_low: u32,
    pub(super) latest_code_high: u32,
    pub(super) latest_severity: i32,
    pub(super) latest_action: i32,
    clear_latched_previous: bool,
}

impl DiagnosticState {
    pub(super) fn new() -> Self {
        Self {
            latest_code_domain: -1,
            ..Self::default()
        }
    }

    pub(super) fn update(&mut self, report: &DiagnosticReport, clear_latched: bool) {
        if clear_latched && !self.clear_latched_previous {
            self.latched_error_mask = 0;
            self.latched_warning_mask = 0;
        }
        self.clear_latched_previous = clear_latched;
        self.latched_error_mask |= report.active_error_mask;
        self.latched_warning_mask |= report.active_warning_mask;

        let transition = self.logger.update(report);
        self.transitions = self.transitions.wrapping_add(transition.count);
        if let Some(issue) = transition.latest {
            let value = issue.value as u64;
            self.latest_code_domain = if issue.domain_id == u32::MAX {
                -1
            } else {
                issue.domain_id as i32
            };
            self.latest_code_low = value as u32;
            self.latest_code_high = (value >> 32) as u32;
            self.latest_severity = match issue.severity {
                Severity::Warning => 1,
                Severity::Error => 2,
            };
            self.latest_action = transition.latest_action;
        }
    }
}
