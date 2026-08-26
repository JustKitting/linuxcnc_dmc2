use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::CString;
use std::rc::Rc;
use std::time::{Duration, Instant};

use dmc2_linuxcnc_interface::ERROR_MESSAGE_CONTRACTS;

use crate::application::diagnostic_state::DiagnosticState;
use crate::application::error_channel::{
    ErrorChannelFault, ErrorChannelRead, ErrorMessageRecord, ErrorSeverity,
};
use crate::application::nml::{PollCodes, PollDisposition, PollOutcome, TransportStatus};
use crate::diagnostics::{category, DiagnosticReport};
use crate::snapshot::NativeSnapshot;

use super::contracts::{
    ErrorConnector, ErrorReader, HalSink, JournalSink, RuntimeReporter, StatusConnector,
    StatusReader,
};
use super::coordinator::{RuntimeCoordinator, MAX_ERROR_MESSAGES_PER_CYCLE, RECONNECT_PERIOD};

#[derive(Default)]
struct Trace {
    status_opens: usize,
    status_polls: usize,
    status_drops: usize,
    error_opens: usize,
    error_reads: usize,
    error_drops: usize,
}

type SharedTrace = Rc<RefCell<Trace>>;

#[derive(Clone)]
struct StatusEvent {
    outcome: PollOutcome,
    valid_snapshot: bool,
    heartbeat: u32,
}

struct StatusOpen {
    present: bool,
    transport: TransportStatus,
    events: VecDeque<StatusEvent>,
}

struct FakeStatusConnector {
    trace: SharedTrace,
    opens: VecDeque<StatusOpen>,
}

struct FakeStatusReader {
    trace: SharedTrace,
    events: VecDeque<StatusEvent>,
}

impl Drop for FakeStatusReader {
    fn drop(&mut self) {
        self.trace.borrow_mut().status_drops += 1;
    }
}

impl StatusReader for FakeStatusReader {
    fn poll(&mut self, snapshot: &mut NativeSnapshot, _codes: PollCodes) -> PollOutcome {
        self.trace.borrow_mut().status_polls += 1;
        let event = self
            .events
            .pop_front()
            .expect("unexpected status poll in runtime test");
        *snapshot = NativeSnapshot::safe();
        snapshot.task.heartbeat = event.heartbeat;
        if !event.valid_snapshot {
            snapshot.abi_version ^= 1;
        }
        event.outcome
    }
}

impl StatusConnector for FakeStatusConnector {
    type Reader = FakeStatusReader;

    fn open(
        &mut self,
        _nml_file: &CString,
        _codes: PollCodes,
    ) -> (Option<Self::Reader>, TransportStatus) {
        self.trace.borrow_mut().status_opens += 1;
        let open = self
            .opens
            .pop_front()
            .expect("unexpected status open in runtime test");
        let reader = open.present.then(|| FakeStatusReader {
            trace: Rc::clone(&self.trace),
            events: open.events,
        });
        (reader, open.transport)
    }
}

struct ErrorOpen {
    present: bool,
    transport: TransportStatus,
    events: VecDeque<ErrorChannelRead>,
}

struct FakeErrorConnector {
    trace: SharedTrace,
    opens: VecDeque<ErrorOpen>,
}

struct FakeErrorReader {
    trace: SharedTrace,
    events: VecDeque<ErrorChannelRead>,
    empty_transport: TransportStatus,
}

impl Drop for FakeErrorReader {
    fn drop(&mut self) {
        self.trace.borrow_mut().error_drops += 1;
    }
}

impl ErrorReader for FakeErrorReader {
    fn read(&mut self, _codes: PollCodes) -> ErrorChannelRead {
        self.trace.borrow_mut().error_reads += 1;
        self.events
            .pop_front()
            .unwrap_or(ErrorChannelRead::Empty(self.empty_transport))
    }
}

impl ErrorConnector for FakeErrorConnector {
    type Reader = FakeErrorReader;

    fn open(
        &mut self,
        _nml_file: &CString,
        codes: PollCodes,
    ) -> (Option<Self::Reader>, TransportStatus) {
        self.trace.borrow_mut().error_opens += 1;
        let open = self
            .opens
            .pop_front()
            .expect("unexpected error-channel open in runtime test");
        let reader = open.present.then(|| FakeErrorReader {
            trace: Rc::clone(&self.trace),
            events: open.events,
            empty_transport: TransportStatus {
                nml_error: codes.no_error,
                cms_status: codes.cms_read_old,
            },
        });
        (reader, open.transport)
    }
}

#[derive(Default)]
struct FakeJournal {
    attempts: usize,
    fail_at: Option<usize>,
    records: Vec<(TransportStatus, ErrorMessageRecord)>,
}

impl JournalSink for FakeJournal {
    fn append(
        &mut self,
        transport: TransportStatus,
        record: &ErrorMessageRecord,
    ) -> Result<u64, String> {
        self.attempts += 1;
        if self.fail_at == Some(self.attempts) {
            return Err(format!("journal failure at {}", self.attempts));
        }
        self.records.push((transport, record.clone()));
        Ok(self.records.len() as u64)
    }
}

#[derive(Clone)]
struct Publication {
    snapshot: NativeSnapshot,
    connected: bool,
    fault: bool,
    transport: TransportStatus,
    diagnostics: DiagnosticReport,
}

#[derive(Default)]
struct FakeHal {
    poll_errors: usize,
    publications: Vec<Publication>,
}

impl HalSink for FakeHal {
    fn increment_poll_errors(&mut self) {
        self.poll_errors += 1;
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
        diagnostic_state.update(diagnostics, false);
        self.publications.push(Publication {
            snapshot,
            connected,
            fault,
            transport,
            diagnostics: diagnostics.clone(),
        });
    }
}

#[derive(Default)]
struct FakeReporter {
    messages: Vec<(u64, ErrorMessageRecord)>,
    faults: Vec<ErrorChannelFault>,
}

impl RuntimeReporter for FakeReporter {
    fn message(&mut self, sequence: u64, record: &ErrorMessageRecord) {
        self.messages.push((sequence, record.clone()));
    }

    fn error_channel_fault(&mut self, fault: &ErrorChannelFault) {
        self.faults.push(fault.clone());
    }
}

type TestRuntime =
    RuntimeCoordinator<FakeStatusConnector, FakeErrorConnector, FakeJournal, FakeHal, FakeReporter>;

fn healthy_open(codes: PollCodes) -> TransportStatus {
    TransportStatus {
        nml_error: codes.no_error,
        cms_status: codes.cms_status_not_set,
    }
}

fn failed_transport(codes: PollCodes) -> TransportStatus {
    TransportStatus {
        nml_error: codes.invalid_configuration,
        cms_status: codes.cms_status_not_set,
    }
}

fn status_transport(codes: PollCodes, disposition: PollDisposition) -> TransportStatus {
    TransportStatus {
        nml_error: if disposition == PollDisposition::Fault {
            codes.invalid_configuration
        } else {
            codes.no_error
        },
        cms_status: if disposition == PollDisposition::WaitingForFirstStatus {
            codes.cms_read_old
        } else {
            codes.cms_read_ok
        },
    }
}

fn status_event(
    codes: PollCodes,
    disposition: PollDisposition,
    valid_snapshot: bool,
    heartbeat: u32,
) -> StatusEvent {
    StatusEvent {
        outcome: PollOutcome {
            disposition,
            transport: status_transport(codes, disposition),
        },
        valid_snapshot,
        heartbeat,
    }
}

fn status_open(
    present: bool,
    transport: TransportStatus,
    events: impl IntoIterator<Item = StatusEvent>,
) -> StatusOpen {
    StatusOpen {
        present,
        transport,
        events: events.into_iter().collect(),
    }
}

fn error_open(
    present: bool,
    transport: TransportStatus,
    events: impl IntoIterator<Item = ErrorChannelRead>,
) -> ErrorOpen {
    ErrorOpen {
        present,
        transport,
        events: events.into_iter().collect(),
    }
}

fn record(index: usize, marker: u8) -> ErrorMessageRecord {
    let contract = ERROR_MESSAGE_CONTRACTS[index];
    let mut object = [0; 280];
    object[..4].copy_from_slice(&(contract.message_type as i32).to_ne_bytes());
    object[8..16].copy_from_slice(&(contract.message_size as i64).to_ne_bytes());
    let payload = vec![marker; contract.payload_size];
    object[contract.payload_offset..contract.payload_offset + contract.payload_size]
        .copy_from_slice(&payload);
    ErrorMessageRecord {
        message_type: contract.message_type as i32,
        contract: Some(contract),
        severity: if contract.class_name.ends_with("_ERROR") {
            ErrorSeverity::Error
        } else {
            ErrorSeverity::Info
        },
        object_size: contract.message_size,
        declared_size: contract.message_size as i64,
        serial_number: contract.serial_offset.map(|_| i32::from(marker)),
        operator_id: contract.id_offset.map(|_| -i32::from(marker)),
        payload: payload.clone(),
        text: payload,
        padding: vec![
            0;
            contract.message_size
                - contract.type_size
                - contract.size_size
                - contract.serial_size
                - contract.id_size
                - contract.payload_size
        ],
        object,
    }
}

fn runtime(
    now: Instant,
    status_opens: impl IntoIterator<Item = StatusOpen>,
    error_opens: impl IntoIterator<Item = ErrorOpen>,
    journal: FakeJournal,
) -> (TestRuntime, SharedTrace) {
    let trace = Rc::new(RefCell::new(Trace::default()));
    let runtime = RuntimeCoordinator::new(
        CString::new("test.nml").unwrap(),
        PollCodes::required(),
        FakeStatusConnector {
            trace: Rc::clone(&trace),
            opens: status_opens.into_iter().collect(),
        },
        FakeErrorConnector {
            trace: Rc::clone(&trace),
            opens: error_opens.into_iter().collect(),
        },
        journal,
        FakeHal::default(),
        FakeReporter::default(),
        now,
    );
    (runtime, trace)
}

#[test]
fn every_open_presence_and_transport_pair_has_one_exact_result() {
    let codes = PollCodes::required();
    let now = Instant::now();
    for status_present in [false, true] {
        for status_healthy in [false, true] {
            for error_present in [false, true] {
                for error_healthy in [false, true] {
                    let status_accepted = status_present && status_healthy;
                    let error_accepted = error_present && error_healthy;
                    let (mut runtime, trace) = runtime(
                        now,
                        [status_open(
                            status_present,
                            if status_healthy {
                                healthy_open(codes)
                            } else {
                                failed_transport(codes)
                            },
                            [status_event(
                                codes,
                                PollDisposition::WaitingForFirstStatus,
                                true,
                                0,
                            )],
                        )],
                        [error_open(
                            error_present,
                            if error_healthy {
                                healthy_open(codes)
                            } else {
                                failed_transport(codes)
                            },
                            [],
                        )],
                        FakeJournal::default(),
                    );
                    runtime.cycle(now).unwrap();
                    assert_eq!(
                        runtime.connection_state(),
                        (status_accepted, error_accepted),
                        "status present={status_present} healthy={status_healthy}; error present={error_present} healthy={error_healthy}"
                    );
                    let (status_retry, error_retry) = runtime.retry_deadlines();
                    assert_eq!(
                        status_retry,
                        if status_accepted {
                            now
                        } else {
                            now + RECONNECT_PERIOD
                        }
                    );
                    assert_eq!(
                        error_retry,
                        if error_accepted {
                            now
                        } else {
                            now + RECONNECT_PERIOD
                        }
                    );
                    let (_, _, _, hal, _) = runtime.into_parts();
                    assert_eq!(
                        hal.poll_errors,
                        usize::from(!status_accepted) + usize::from(!error_accepted)
                    );
                    let trace = trace.borrow();
                    assert_eq!(trace.status_opens, 1);
                    assert_eq!(trace.error_opens, 1);
                    assert_eq!(trace.status_polls, usize::from(status_accepted));
                    assert_eq!(trace.error_reads, usize::from(error_accepted));
                    assert_eq!(
                        trace.status_drops,
                        usize::from(status_present && !status_healthy)
                            + usize::from(status_accepted)
                    );
                    assert_eq!(
                        trace.error_drops,
                        usize::from(error_present && !error_healthy) + usize::from(error_accepted)
                    );
                }
            }
        }
    }
}

#[test]
fn reconnects_are_independent_and_run_only_at_the_exact_deadline() {
    let codes = PollCodes::required();
    let now = Instant::now();
    let just_before = now + RECONNECT_PERIOD - Duration::from_nanos(1);
    let deadline = now + RECONNECT_PERIOD;
    let (mut runtime, trace) = runtime(
        now,
        [
            status_open(false, failed_transport(codes), []),
            status_open(
                true,
                healthy_open(codes),
                [status_event(
                    codes,
                    PollDisposition::WaitingForFirstStatus,
                    true,
                    0,
                )],
            ),
        ],
        [
            error_open(false, failed_transport(codes), []),
            error_open(true, healthy_open(codes), []),
        ],
        FakeJournal::default(),
    );
    runtime.cycle(now).unwrap();
    runtime.cycle(just_before).unwrap();
    assert_eq!(trace.borrow().status_opens, 1);
    assert_eq!(trace.borrow().error_opens, 1);
    runtime.cycle(deadline).unwrap();
    assert_eq!(runtime.connection_state(), (true, true));
    assert_eq!(trace.borrow().status_opens, 2);
    assert_eq!(trace.borrow().error_opens, 2);
}

#[test]
fn established_channel_faults_reconnect_only_the_failed_channel() {
    let codes = PollCodes::required();
    let now = Instant::now();
    let before_retry = now + RECONNECT_PERIOD - Duration::from_nanos(1);
    let retry = now + RECONNECT_PERIOD;

    let (mut error_runtime, error_trace) = runtime(
        now,
        [status_open(
            true,
            healthy_open(codes),
            [
                status_event(codes, PollDisposition::Snapshot, true, 1),
                status_event(codes, PollDisposition::Snapshot, true, 2),
                status_event(codes, PollDisposition::Snapshot, true, 3),
            ],
        )],
        [
            error_open(
                true,
                healthy_open(codes),
                [ErrorChannelRead::Fault(ErrorChannelFault::test_fault(
                    failed_transport(codes),
                ))],
            ),
            error_open(true, healthy_open(codes), []),
        ],
        FakeJournal::default(),
    );
    error_runtime.cycle(now).unwrap();
    assert_eq!(error_runtime.connection_state(), (true, false));
    error_runtime.cycle(before_retry).unwrap();
    assert_eq!(error_runtime.connection_state(), (true, false));
    error_runtime.cycle(retry).unwrap();
    assert_eq!(error_runtime.connection_state(), (true, true));
    assert_eq!(error_trace.borrow().status_opens, 1);
    assert_eq!(error_trace.borrow().error_opens, 2);

    let later = retry + Duration::from_secs(5);
    let (mut status_runtime, status_trace) = runtime(
        later,
        [
            status_open(
                true,
                healthy_open(codes),
                [status_event(codes, PollDisposition::Fault, true, 1)],
            ),
            status_open(
                true,
                healthy_open(codes),
                [status_event(
                    codes,
                    PollDisposition::WaitingForFirstStatus,
                    true,
                    0,
                )],
            ),
        ],
        [error_open(true, healthy_open(codes), [])],
        FakeJournal::default(),
    );
    status_runtime.cycle(later).unwrap();
    assert_eq!(status_runtime.connection_state(), (false, true));
    status_runtime
        .cycle(later + RECONNECT_PERIOD - Duration::from_nanos(1))
        .unwrap();
    assert_eq!(status_runtime.connection_state(), (false, true));
    status_runtime.cycle(later + RECONNECT_PERIOD).unwrap();
    assert_eq!(status_runtime.connection_state(), (true, true));
    assert_eq!(status_trace.borrow().status_opens, 2);
    assert_eq!(status_trace.borrow().error_opens, 1);
}

#[test]
fn coordinator_executes_every_publication_policy_branch_exactly() {
    let codes = PollCodes::required();
    let now = Instant::now();
    for disposition in [
        PollDisposition::WaitingForFirstStatus,
        PollDisposition::Snapshot,
        PollDisposition::Fault,
    ] {
        for valid_snapshot in [false, true] {
            for error_connected in [false, true] {
                let (mut runtime, _) = runtime(
                    now,
                    [status_open(
                        true,
                        healthy_open(codes),
                        [status_event(codes, disposition, valid_snapshot, 0x5a5a)],
                    )],
                    [error_open(
                        error_connected,
                        if error_connected {
                            healthy_open(codes)
                        } else {
                            failed_transport(codes)
                        },
                        [],
                    )],
                    FakeJournal::default(),
                );
                runtime.cycle(now).unwrap();
                let expected_live =
                    disposition == PollDisposition::Snapshot && valid_snapshot && error_connected;
                let expected_wait =
                    disposition == PollDisposition::WaitingForFirstStatus && error_connected;
                let expected_drop = disposition == PollDisposition::Fault
                    || (disposition == PollDisposition::Snapshot && !valid_snapshot);
                assert_eq!(
                    runtime.connection_state().0,
                    !expected_drop,
                    "disposition={disposition:?} valid={valid_snapshot} error_connected={error_connected}"
                );
                let (_, _, _, hal, _) = runtime.into_parts();
                assert_eq!(hal.publications.len(), usize::from(!expected_wait));
                if let Some(publication) = hal.publications.first() {
                    assert_eq!(publication.connected, expected_live);
                    assert_eq!(publication.fault, !expected_live);
                    assert_eq!(
                        publication.snapshot.task.heartbeat,
                        if expected_live { 0x5a5a } else { 0 }
                    );
                    assert!(publication.snapshot.valid_abi());
                    if !expected_live {
                        assert!(publication.snapshot.io.aux.estop != 0);
                        assert!(publication.diagnostics.error_active());
                    }
                    if expected_drop {
                        assert_eq!(publication.transport, status_transport(codes, disposition));
                        let diagnostic_kind = publication.diagnostics.active_error_mask
                            & (category::ABI | category::TRANSPORT);
                        assert_eq!(
                            diagnostic_kind,
                            if disposition == PollDisposition::Snapshot {
                                category::ABI
                            } else {
                                category::TRANSPORT
                            },
                            "disposition={disposition:?} valid={valid_snapshot} error_connected={error_connected}"
                        );
                    }
                }
                assert_eq!(
                    hal.poll_errors,
                    usize::from(!error_connected) + usize::from(expected_drop)
                );
            }
        }
    }
}

#[test]
fn empty_message_and_fault_error_reads_have_exact_distinct_effects() {
    let codes = PollCodes::required();
    let now = Instant::now();
    let read_transport = TransportStatus {
        nml_error: codes.no_error,
        cms_status: codes.cms_read_ok,
    };
    for case in 0..3 {
        let events = match case {
            0 => vec![ErrorChannelRead::Empty(TransportStatus {
                nml_error: codes.no_error,
                cms_status: codes.cms_read_old,
            })],
            1 => vec![
                ErrorChannelRead::Message(read_transport, record(0, 7)),
                ErrorChannelRead::Empty(TransportStatus {
                    nml_error: codes.no_error,
                    cms_status: codes.cms_read_old,
                }),
            ],
            2 => vec![ErrorChannelRead::Fault(ErrorChannelFault::test_fault(
                failed_transport(codes),
            ))],
            _ => unreachable!(),
        };
        let (mut runtime, trace) = runtime(
            now,
            [status_open(
                true,
                healthy_open(codes),
                [status_event(
                    codes,
                    PollDisposition::WaitingForFirstStatus,
                    true,
                    0,
                )],
            )],
            [error_open(true, healthy_open(codes), events)],
            FakeJournal::default(),
        );
        runtime.cycle(now).unwrap();
        assert_eq!(runtime.connection_state().1, case != 2);
        let (_, _, journal, hal, reporter) = runtime.into_parts();
        assert_eq!(journal.records.len(), usize::from(case == 1));
        assert_eq!(reporter.messages.len(), usize::from(case == 1));
        assert_eq!(reporter.faults.len(), usize::from(case == 2));
        assert_eq!(hal.poll_errors, usize::from(case == 2));
        assert_eq!(hal.publications.len(), usize::from(case == 2));
        assert_eq!(trace.borrow().error_reads, [1, 2, 1][case]);
    }
}

#[test]
fn drain_limit_defers_without_dropping_reordering_or_duplicating_messages() {
    let codes = PollCodes::required();
    let now = Instant::now();
    let read_transport = TransportStatus {
        nml_error: codes.no_error,
        cms_status: codes.cms_read_ok,
    };
    let messages = (0..=MAX_ERROR_MESSAGES_PER_CYCLE)
        .map(|index| ErrorChannelRead::Message(read_transport, record(index % 6, index as u8)))
        .collect::<Vec<_>>();
    let (mut runtime, trace) = runtime(
        now,
        [status_open(
            true,
            healthy_open(codes),
            [
                status_event(codes, PollDisposition::WaitingForFirstStatus, true, 0),
                status_event(codes, PollDisposition::WaitingForFirstStatus, true, 0),
            ],
        )],
        [error_open(true, healthy_open(codes), messages)],
        FakeJournal::default(),
    );
    runtime.cycle(now).unwrap();
    assert_eq!(trace.borrow().error_reads, MAX_ERROR_MESSAGES_PER_CYCLE);
    runtime.cycle(now + Duration::from_millis(10)).unwrap();
    assert_eq!(trace.borrow().error_reads, MAX_ERROR_MESSAGES_PER_CYCLE + 2);
    let (_, _, journal, _, reporter) = runtime.into_parts();
    assert_eq!(journal.records.len(), MAX_ERROR_MESSAGES_PER_CYCLE + 1);
    assert_eq!(reporter.messages.len(), MAX_ERROR_MESSAGES_PER_CYCLE + 1);
    for (index, ((_, journal_record), (sequence, reported_record))) in journal
        .records
        .iter()
        .zip(reporter.messages.iter())
        .enumerate()
    {
        assert_eq!(*sequence, index as u64 + 1);
        assert_eq!(journal_record, reported_record);
        assert_eq!(journal_record.payload[0], index as u8);
    }
}

#[test]
fn every_journal_failure_position_fails_closed_before_reporting_that_record() {
    let codes = PollCodes::required();
    let now = Instant::now();
    let read_transport = TransportStatus {
        nml_error: codes.no_error,
        cms_status: codes.cms_read_ok,
    };
    for fail_at in 1..=MAX_ERROR_MESSAGES_PER_CYCLE + 1 {
        let messages = (0..fail_at)
            .map(|index| ErrorChannelRead::Message(read_transport, record(index % 6, index as u8)))
            .collect::<Vec<_>>();
        let status_events = [
            status_event(codes, PollDisposition::WaitingForFirstStatus, true, 0),
            status_event(codes, PollDisposition::WaitingForFirstStatus, true, 0),
        ];
        let (mut runtime, _) = runtime(
            now,
            [status_open(true, healthy_open(codes), status_events)],
            [error_open(true, healthy_open(codes), messages)],
            FakeJournal {
                fail_at: Some(fail_at),
                ..FakeJournal::default()
            },
        );
        if fail_at > MAX_ERROR_MESSAGES_PER_CYCLE {
            runtime.cycle(now).unwrap();
        }
        let error = runtime.cycle(now + Duration::from_millis(10)).unwrap_err();
        assert_eq!(error, format!("journal failure at {fail_at}"));
        assert_eq!(runtime.connection_state(), (false, false));
        let (_, _, journal, hal, reporter) = runtime.into_parts();
        assert_eq!(journal.attempts, fail_at);
        assert_eq!(journal.records.len(), fail_at - 1);
        assert_eq!(reporter.messages.len(), fail_at - 1);
        assert_eq!(hal.poll_errors, 1);
        let publication = hal.publications.last().unwrap();
        assert!(!publication.connected);
        assert!(publication.fault);
        assert!(publication.snapshot.io.aux.estop != 0);
        assert_eq!(publication.transport, failed_transport(codes));
    }
}
