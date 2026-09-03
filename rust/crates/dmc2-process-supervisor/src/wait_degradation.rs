use std::io;

use crate::event::{hex_bytes, Event};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoErrorEvidence {
    pub kind: io::ErrorKind,
    pub raw_os_error: Option<i32>,
    pub detail: String,
}

impl IoErrorEvidence {
    pub fn capture(error: &io::Error) -> Self {
        Self {
            kind: error.kind(),
            raw_os_error: error.raw_os_error(),
            detail: error.to_string(),
        }
    }

    pub fn waitid_event_fields(&self, event: Event) -> Event {
        event
            .field("error_kind", format!("{:?}", self.kind))
            .field("raw_os_error", optional_i32(self.raw_os_error))
            .field("error_hex", hex_bytes(self.detail.as_bytes()))
    }

    pub fn fallback_event_fields(&self, event: Event) -> Event {
        event
            .field("fallback_wait4_error_kind", format!("{:?}", self.kind))
            .field(
                "fallback_wait4_raw_os_error",
                optional_i32(self.raw_os_error),
            )
            .field(
                "fallback_wait4_error_hex",
                hex_bytes(self.detail.as_bytes()),
            )
    }

    fn fallback_summary_fields(&self, event: Event) -> Event {
        event
            .field("last_fallback_wait4_error_kind", format!("{:?}", self.kind))
            .field(
                "last_fallback_wait4_raw_os_error",
                optional_i32(self.raw_os_error),
            )
            .field(
                "last_fallback_wait4_error_hex",
                hex_bytes(self.detail.as_bytes()),
            )
    }
}

#[derive(Clone)]
pub struct WaitDegradation {
    waitid_error: IoErrorEvidence,
    fallback_wait4_failures: u64,
    fallback_wait4_error_transitions: u64,
    fallback_wait4_recoveries: u64,
    active_fallback_wait4_error: Option<IoErrorEvidence>,
    last_fallback_wait4_error: Option<IoErrorEvidence>,
}

impl WaitDegradation {
    pub fn new(waitid_error: io::Error) -> Self {
        Self {
            waitid_error: IoErrorEvidence::capture(&waitid_error),
            fallback_wait4_failures: 0,
            fallback_wait4_error_transitions: 0,
            fallback_wait4_recoveries: 0,
            active_fallback_wait4_error: None,
            last_fallback_wait4_error: None,
        }
    }

    pub fn record_fallback_failure(&mut self, error: &io::Error) -> Option<IoErrorEvidence> {
        self.fallback_wait4_failures = self.fallback_wait4_failures.saturating_add(1);
        let evidence = IoErrorEvidence::capture(error);
        if self.active_fallback_wait4_error.as_ref() == Some(&evidence) {
            return None;
        }
        self.fallback_wait4_error_transitions =
            self.fallback_wait4_error_transitions.saturating_add(1);
        self.active_fallback_wait4_error = Some(evidence.clone());
        self.last_fallback_wait4_error = Some(evidence.clone());
        Some(evidence)
    }

    pub fn record_fallback_success(&mut self) -> bool {
        if self.active_fallback_wait4_error.take().is_none() {
            return false;
        }
        self.fallback_wait4_recoveries = self.fallback_wait4_recoveries.saturating_add(1);
        true
    }

    pub fn waitid_event_fields(&self, event: Event) -> Event {
        self.waitid_error.waitid_event_fields(event)
    }

    pub fn fallback_wait4_failures(&self) -> u64 {
        self.fallback_wait4_failures
    }

    pub fn waitid_error(&self) -> &IoErrorEvidence {
        &self.waitid_error
    }

    pub fn summary_event_fields(&self, event: Event) -> Event {
        let event = event
            .field("terminal_observation_degraded", true)
            .field(
                "terminal_waitid_error_kind",
                format!("{:?}", self.waitid_error.kind),
            )
            .field(
                "terminal_waitid_raw_os_error",
                optional_i32(self.waitid_error.raw_os_error),
            )
            .field(
                "terminal_waitid_error_hex",
                hex_bytes(self.waitid_error.detail.as_bytes()),
            )
            .field("fallback_wait4_failures", self.fallback_wait4_failures)
            .field(
                "fallback_wait4_error_transitions",
                self.fallback_wait4_error_transitions,
            )
            .field("fallback_wait4_recoveries", self.fallback_wait4_recoveries)
            .field(
                "fallback_wait4_error_active_at_terminal",
                self.active_fallback_wait4_error.is_some(),
            );
        match &self.last_fallback_wait4_error {
            Some(error) => error.fallback_summary_fields(event),
            None => event
                .field("last_fallback_wait4_error_kind", "NONE")
                .field("last_fallback_wait4_raw_os_error", "NONE")
                .field("last_fallback_wait4_error_hex", "NONE"),
        }
    }
}

fn optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "NONE".to_owned(), |value| value.to_string())
}
