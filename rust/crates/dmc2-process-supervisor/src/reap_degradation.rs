use std::io;

use crate::event::{hex_bytes, Event};
use crate::wait_degradation::IoErrorEvidence;

pub struct ReapDegradation {
    first_error: IoErrorEvidence,
    attempts: u64,
    failures: u64,
    error_transitions: u64,
    recoveries: u64,
    active_error: Option<IoErrorEvidence>,
    last_error: IoErrorEvidence,
}

impl ReapDegradation {
    pub fn new(error: &io::Error) -> Self {
        let evidence = IoErrorEvidence::capture(error);
        Self {
            first_error: evidence.clone(),
            attempts: 1,
            failures: 1,
            error_transitions: 1,
            recoveries: 0,
            active_error: Some(evidence.clone()),
            last_error: evidence,
        }
    }

    pub fn record_failure(&mut self, error: &io::Error) -> bool {
        self.attempts = self.attempts.saturating_add(1);
        self.failures = self.failures.saturating_add(1);
        let evidence = IoErrorEvidence::capture(error);
        if self.active_error.as_ref() == Some(&evidence) {
            return false;
        }
        self.error_transitions = self.error_transitions.saturating_add(1);
        self.active_error = Some(evidence.clone());
        self.last_error = evidence;
        true
    }

    pub fn record_success(&mut self) -> bool {
        self.attempts = self.attempts.saturating_add(1);
        if self.active_error.take().is_none() {
            return false;
        }
        self.recoveries = self.recoveries.saturating_add(1);
        true
    }

    pub fn first_error(&self) -> &IoErrorEvidence {
        &self.first_error
    }

    pub fn failures(&self) -> u64 {
        self.failures
    }

    pub fn event_fields(&self, event: Event, source: &io::Error) -> Event {
        let error = IoErrorEvidence::capture(source);
        event
            .field("reap_wait4_error_kind", format!("{:?}", error.kind))
            .field("reap_wait4_raw_os_error", optional_i32(error.raw_os_error))
            .field("reap_wait4_error_hex", hex_bytes(error.detail.as_bytes()))
    }

    pub fn summary_event_fields(&self, event: Event) -> Event {
        event
            .field("terminal_reap_degraded", true)
            .field("reap_wait4_attempts", self.attempts)
            .field("reap_wait4_failures", self.failures)
            .field("reap_wait4_error_transitions", self.error_transitions)
            .field("reap_wait4_recoveries", self.recoveries)
            .field(
                "reap_wait4_error_active_at_terminal",
                self.active_error.is_some(),
            )
            .field(
                "first_reap_wait4_error_kind",
                format!("{:?}", self.first_error.kind),
            )
            .field(
                "first_reap_wait4_raw_os_error",
                optional_i32(self.first_error.raw_os_error),
            )
            .field(
                "first_reap_wait4_error_hex",
                hex_bytes(self.first_error.detail.as_bytes()),
            )
            .field(
                "last_reap_wait4_error_kind",
                format!("{:?}", self.last_error.kind),
            )
            .field(
                "last_reap_wait4_raw_os_error",
                optional_i32(self.last_error.raw_os_error),
            )
            .field(
                "last_reap_wait4_error_hex",
                hex_bytes(self.last_error.detail.as_bytes()),
            )
    }
}

pub fn terminal_became_nonterminal_error(pid: u32) -> io::Error {
    io::Error::other(format!(
        "wait4(WNOHANG) reported child PID {pid} as running after waitid reported it terminal"
    ))
}

fn optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "NONE".to_owned(), |value| value.to_string())
}
