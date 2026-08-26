use std::ffi::CString;
use std::time::{Duration, Instant};

use crate::application::diagnostic_state::DiagnosticState;
use crate::application::error_channel::ErrorChannelRead;
use crate::application::nml::{PollCodes, PollDisposition, TransportStatus};
use crate::diagnostics;
use crate::snapshot::NativeSnapshot;

use super::contracts::{
    ErrorConnector, ErrorReader, HalSink, JournalSink, RuntimeReporter, StatusConnector,
    StatusReader,
};
use super::policy::{publication_policy, PublicationPolicy};

pub(super) const RECONNECT_PERIOD: Duration = Duration::from_secs(1);
pub(super) const MAX_ERROR_MESSAGES_PER_CYCLE: usize = 64;

pub(super) struct RuntimeCoordinator<SC, EC, J, H, R>
where
    SC: StatusConnector,
    EC: ErrorConnector,
    J: JournalSink,
    H: HalSink,
    R: RuntimeReporter,
{
    nml_file: CString,
    codes: PollCodes,
    status_connector: SC,
    error_connector: EC,
    status_reader: Option<SC::Reader>,
    error_reader: Option<EC::Reader>,
    journal: J,
    hal: H,
    reporter: R,
    diagnostic_state: DiagnosticState,
    next_status_open: Instant,
    next_error_open: Instant,
    fault_transport: TransportStatus,
}

impl<SC, EC, J, H, R> RuntimeCoordinator<SC, EC, J, H, R>
where
    SC: StatusConnector,
    EC: ErrorConnector,
    J: JournalSink,
    H: HalSink,
    R: RuntimeReporter,
{
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        nml_file: CString,
        codes: PollCodes,
        status_connector: SC,
        error_connector: EC,
        journal: J,
        hal: H,
        reporter: R,
        now: Instant,
    ) -> Self {
        Self {
            nml_file,
            codes,
            status_connector,
            error_connector,
            status_reader: None,
            error_reader: None,
            journal,
            hal,
            reporter,
            diagnostic_state: DiagnosticState::new(),
            next_status_open: now,
            next_error_open: now,
            fault_transport: TransportStatus {
                nml_error: codes.invalid_configuration,
                cms_status: codes.cms_status_not_set,
            },
        }
    }

    pub(super) fn cycle(&mut self, now: Instant) -> Result<(), String> {
        self.open_due_channels(now);
        self.drain_error_channel(now)?;

        let mut snapshot = NativeSnapshot::safe();
        let status_outcome = self
            .status_reader
            .as_mut()
            .map(|reader| reader.poll(&mut snapshot, self.codes));
        let policy = publication_policy(
            status_outcome.map(|outcome| outcome.disposition),
            snapshot.valid_abi(),
            self.error_reader.is_some(),
        );
        match policy {
            PublicationPolicy::Live => {
                let outcome = status_outcome.expect("live policy requires a status outcome");
                let report = diagnostics::evaluate_with_transport(
                    &snapshot,
                    outcome.transport.nml_error,
                    outcome.transport.cms_status,
                );
                self.hal.publish(
                    snapshot,
                    true,
                    false,
                    outcome.transport,
                    &report,
                    &mut self.diagnostic_state,
                );
            }
            PublicationPolicy::WaitForFirstStatus => {}
            PublicationPolicy::SafeKeepStatus => self.publish_safe(),
            PublicationPolicy::SafeDropStatus => {
                let outcome = status_outcome.expect("drop policy requires a status outcome");
                self.fault_transport = outcome.transport;
                self.status_reader = None;
                self.next_status_open = now + RECONNECT_PERIOD;
                self.hal.increment_poll_errors();
                let report = if outcome.disposition == PollDisposition::Snapshot {
                    diagnostics::evaluate_with_transport(
                        &snapshot,
                        outcome.transport.nml_error,
                        outcome.transport.cms_status,
                    )
                } else {
                    diagnostics::disconnected(
                        outcome.transport.nml_error,
                        outcome.transport.cms_status,
                    )
                };
                self.hal.publish(
                    NativeSnapshot::safe(),
                    false,
                    true,
                    outcome.transport,
                    &report,
                    &mut self.diagnostic_state,
                );
            }
        }
        Ok(())
    }

    fn open_due_channels(&mut self, now: Instant) {
        if self.status_reader.is_none() && now >= self.next_status_open {
            let (opened, transport) = self.status_connector.open(&self.nml_file, self.codes);
            if opened.is_some() && transport.healthy_after_open(self.codes) {
                self.status_reader = opened;
            } else {
                self.fault_transport = transport;
                self.hal.increment_poll_errors();
                self.next_status_open = now + RECONNECT_PERIOD;
            }
        }
        if self.error_reader.is_none() && now >= self.next_error_open {
            let (opened, transport) = self.error_connector.open(&self.nml_file, self.codes);
            if opened.is_some() && transport.healthy_after_open(self.codes) {
                self.error_reader = opened;
            } else {
                self.fault_transport = transport;
                self.hal.increment_poll_errors();
                self.next_error_open = now + RECONNECT_PERIOD;
            }
        }
    }

    fn drain_error_channel(&mut self, now: Instant) -> Result<(), String> {
        let mut error_fault = None;
        if let Some(reader) = self.error_reader.as_mut() {
            for _ in 0..MAX_ERROR_MESSAGES_PER_CYCLE {
                match reader.read(self.codes) {
                    ErrorChannelRead::Empty(_) => break,
                    ErrorChannelRead::Message(transport, record) => {
                        let sequence = match self.journal.append(transport, &record) {
                            Ok(sequence) => sequence,
                            Err(error) => return self.fail_closed(error),
                        };
                        self.reporter.message(sequence, &record);
                    }
                    ErrorChannelRead::Fault(fault) => {
                        error_fault = Some(fault);
                        break;
                    }
                }
            }
        }
        if let Some(fault) = error_fault {
            self.fault_transport = fault.transport;
            self.reporter.error_channel_fault(&fault);
            self.error_reader = None;
            self.next_error_open = now + RECONNECT_PERIOD;
            self.hal.increment_poll_errors();
        }
        Ok(())
    }

    fn fail_closed<T>(&mut self, error: String) -> Result<T, String> {
        self.status_reader = None;
        self.error_reader = None;
        self.fault_transport = TransportStatus {
            nml_error: self.codes.invalid_configuration,
            cms_status: self.codes.cms_status_not_set,
        };
        self.hal.increment_poll_errors();
        self.publish_safe();
        Err(error)
    }

    fn publish_safe(&mut self) {
        let report = diagnostics::disconnected(
            self.fault_transport.nml_error,
            self.fault_transport.cms_status,
        );
        self.hal.publish(
            NativeSnapshot::safe(),
            false,
            true,
            self.fault_transport,
            &report,
            &mut self.diagnostic_state,
        );
    }

    #[cfg(test)]
    pub(super) fn connection_state(&self) -> (bool, bool) {
        (self.status_reader.is_some(), self.error_reader.is_some())
    }

    #[cfg(test)]
    pub(super) fn retry_deadlines(&self) -> (Instant, Instant) {
        (self.next_status_open, self.next_error_open)
    }

    #[cfg(test)]
    pub(super) fn into_parts(self) -> (SC, EC, J, H, R) {
        (
            self.status_connector,
            self.error_connector,
            self.journal,
            self.hal,
            self.reporter,
        )
    }
}
