use std::ffi::CString;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::thread;
use std::time::{Duration, Instant};

use crate::application::cli::Arguments;
use crate::application::diagnostic_state::DiagnosticState;
use crate::application::error_channel::{
    ErrorChannel, ErrorChannelFault, ErrorChannelRead, ErrorJournal, ErrorMessageRecord,
};
use crate::application::hal::HalPublisher;
use crate::application::nml::{PollCodes, PollOutcome, StatusChannel, TransportStatus};
use crate::diagnostics::DiagnosticReport;
use crate::snapshot::NativeSnapshot;

use super::contracts::{
    ErrorConnector, ErrorReader, HalSink, JournalSink, RuntimeReporter, StatusConnector,
    StatusReader,
};
use super::coordinator::RuntimeCoordinator;

const POLL_PERIOD: Duration = Duration::from_millis(10);

pub(super) struct NativeStatusConnector;

impl StatusReader for StatusChannel {
    fn poll(&mut self, snapshot: &mut NativeSnapshot, codes: PollCodes) -> PollOutcome {
        StatusChannel::poll(self, snapshot, codes)
    }
}

impl StatusConnector for NativeStatusConnector {
    type Reader = StatusChannel;

    fn open(
        &mut self,
        nml_file: &CString,
        codes: PollCodes,
    ) -> (Option<Self::Reader>, TransportStatus) {
        StatusChannel::open(nml_file, codes)
    }
}

pub(super) struct NativeErrorConnector;

impl ErrorReader for ErrorChannel {
    fn read(&mut self, codes: PollCodes) -> ErrorChannelRead {
        ErrorChannel::read(self, codes)
    }
}

impl ErrorConnector for NativeErrorConnector {
    type Reader = ErrorChannel;

    fn open(
        &mut self,
        nml_file: &CString,
        codes: PollCodes,
    ) -> (Option<Self::Reader>, TransportStatus) {
        ErrorChannel::open(nml_file, codes)
    }
}

impl JournalSink for ErrorJournal {
    fn append(
        &mut self,
        transport: TransportStatus,
        record: &ErrorMessageRecord,
    ) -> Result<u64, String> {
        ErrorJournal::append(self, transport, record)
    }
}

impl HalSink for HalPublisher {
    fn increment_poll_errors(&mut self) {
        HalPublisher::increment_poll_errors(self);
    }

    fn publish(
        &mut self,
        snapshot: NativeSnapshot,
        connected: bool,
        fault: bool,
        transport: TransportStatus,
        diagnostics: &DiagnosticReport,
        diagnostic_state: &mut DiagnosticState,
    ) {
        HalPublisher::publish(
            self,
            snapshot,
            connected,
            fault,
            transport,
            diagnostics,
            diagnostic_state,
        );
    }
}

pub(super) struct ConsoleReporter;

impl RuntimeReporter for ConsoleReporter {
    fn message(&mut self, sequence: u64, record: &ErrorMessageRecord) {
        // The synchronized journal is authoritative. Console output is
        // diagnostic-only, so a closed stdout cannot disable the monitor.
        let _ = writeln!(
            io::stdout().lock(),
            "DMC2_LINUXCNC_ERROR_CHANNEL sequence={sequence} kind={} name={} severity={} known={} text={:?}",
            record.message_type,
            record.class_name(),
            record.severity.journal_name(),
            u8::from(record.known()),
            String::from_utf8_lossy(&record.text),
        );
    }

    fn error_channel_fault(&mut self, fault: &ErrorChannelFault) {
        // HAL is published fail-closed independently of diagnostic stderr.
        let _ = writeln!(
            io::stderr().lock(),
            "dmc2-task-monitor: error channel fault: {fault}"
        );
    }
}

pub(in crate::application) fn run(args: Arguments) -> Result<(), String> {
    let error_journal_path = args.error_journal.ok_or_else(|| {
        "runtime task monitor requires --error-journal PATH for lossless error ownership".to_owned()
    })?;
    let error_journal = ErrorJournal::create(&error_journal_path)?;
    let nml_file = CString::new(args.nml_file.as_os_str().as_bytes())
        .map_err(|_| "NML file path contained a NUL byte".to_owned())?;
    let hal = HalPublisher::new(&args.component)?;
    let now = Instant::now();
    let mut runtime = RuntimeCoordinator::new(
        nml_file,
        PollCodes::required(),
        NativeStatusConnector,
        NativeErrorConnector,
        error_journal,
        hal,
        ConsoleReporter,
        now,
    );
    loop {
        runtime.cycle(Instant::now())?;
        thread::sleep(POLL_PERIOD);
    }
}
