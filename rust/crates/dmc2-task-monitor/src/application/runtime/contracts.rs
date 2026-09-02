use std::ffi::CString;

use crate::application::diagnostic_state::DiagnosticState;
use crate::application::error_channel::{ErrorChannelFault, ErrorChannelRead, ErrorMessageRecord};
use crate::application::hal::PublisherError;
use crate::application::journal_error::JournalError;
use crate::application::nml::{PollCodes, PollOutcome, TransportStatus};
use crate::diagnostics::DiagnosticReport;
use crate::snapshot::NativeSnapshot;

pub(super) trait StatusReader {
    fn poll(&mut self, snapshot: &mut NativeSnapshot, codes: PollCodes) -> PollOutcome;
}

pub(super) trait StatusConnector {
    type Reader: StatusReader;

    fn open(
        &mut self,
        nml_file: &CString,
        codes: PollCodes,
    ) -> (Option<Self::Reader>, TransportStatus);
}

pub(super) trait ErrorReader {
    fn read(&mut self, codes: PollCodes) -> ErrorChannelRead;
}

pub(super) trait ErrorConnector {
    type Reader: ErrorReader;

    fn open(
        &mut self,
        nml_file: &CString,
        codes: PollCodes,
    ) -> (Option<Self::Reader>, TransportStatus);
}

pub(super) trait JournalSink {
    fn append(
        &mut self,
        transport: TransportStatus,
        record: &ErrorMessageRecord,
    ) -> Result<u64, JournalError>;
}

pub(super) trait HalSink {
    fn increment_poll_errors(&mut self);

    fn publish(
        &mut self,
        snapshot: NativeSnapshot,
        connected: bool,
        fault: bool,
        transport: TransportStatus,
        diagnostics: &DiagnosticReport,
        diagnostic_state: &mut DiagnosticState,
    ) -> Result<(), PublisherError>;
}

pub(super) trait RuntimeReporter {
    fn message(&mut self, sequence: u64, record: &ErrorMessageRecord);
    fn error_channel_fault(&mut self, fault: &ErrorChannelFault);
}
