mod cli;
mod error;
mod serial;

use std::ffi::CString;
use std::thread;
use std::time::{Duration, Instant};

use dmc2_diagnostics::DiagnosticDisplay;
use dmc2_serial_bridge::{BridgeFaultRecord, BridgeState, ProtocolError};

use self::cli::arguments;
use self::error::ApplicationError;
use self::serial::{LineAssembler, LineEvent, SerialPort};
use dmc2_serial_bridge::hal::HalPublisher;

const READ_PERIOD: Duration = Duration::from_millis(5);
const RECONNECT_PERIOD: Duration = Duration::from_secs(1);

pub(super) fn run() -> Result<(), ApplicationError> {
    let args = arguments()?;
    let timeout_ns =
        args.timeout_ms
            .checked_mul(1_000_000)
            .ok_or(ApplicationError::PacketTimeoutOverflow {
                milliseconds: args.timeout_ms,
            })?;
    let port =
        CString::new(args.port.as_str()).map_err(|error| ApplicationError::SerialPathNul {
            position: error.nul_position(),
        })?;
    let hal = HalPublisher::new(&args.component)?;
    let epoch = Instant::now();
    let mut state = BridgeState::new(timeout_ns);
    let mut reported_fault = None;
    hal.publish(state.snapshot, -1.0);
    report_bridge_transition(&mut reported_fault, state.snapshot.current_fault, None);

    loop {
        let serial = match SerialPort::open(&port, args.baud) {
            Ok(serial) => serial,
            Err(error) => {
                state.note_serial_open_failure(
                    error.operating_system_error(),
                    error.contract_result(),
                );
                report_bridge_transition(
                    &mut reported_fault,
                    state.snapshot.current_fault,
                    Some(&error.to_string()),
                );
                hal.publish(state.snapshot, -1.0);
                thread::sleep(RECONNECT_PERIOD);
                continue;
            }
        };

        let mut assembler = LineAssembler::new();
        let mut buffer = [0_u8; 256];
        loop {
            let count = match serial.read(&mut buffer) {
                Ok(value) => value,
                Err(error) => {
                    state.note_serial_read_failure(
                        error.operating_system_error(),
                        error.contract_result(),
                    );
                    report_bridge_transition(
                        &mut reported_fault,
                        state.snapshot.current_fault,
                        Some(&error.to_string()),
                    );
                    hal.publish(state.snapshot, -1.0);
                    break;
                }
            };
            let now_ns = epoch.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            let mut published_line = false;
            for &byte in &buffer[..count] {
                let event = assembler.consume(byte, &mut state, now_ns);
                if let LineEvent::Rejected(error) = event {
                    report_protocol_error(error, &state);
                }
                report_bridge_transition(&mut reported_fault, state.snapshot.current_fault, None);
                published_line |= event.requires_publish();
            }
            let timed_out = state.check_timeout(now_ns);
            report_bridge_transition(&mut reported_fault, state.snapshot.current_fault, None);
            if published_line || timed_out || count == 0 {
                hal.publish(state.snapshot, state.packet_age_ms(now_ns));
            }
            if timed_out {
                break;
            }
            thread::sleep(READ_PERIOD);
        }
    }
}

fn report_bridge_transition(
    previous: &mut Option<BridgeFaultRecord>,
    current: Option<BridgeFaultRecord>,
    transport_detail: Option<&str>,
) {
    if *previous == current {
        return;
    }
    if let Some(cleared) = *previous {
        eprintln!(
            "dmc2-serial-bridge: transition=clear diagnostic={}",
            DiagnosticDisplay(cleared.code),
        );
    }
    if let Some(record) = current {
        report_bridge_fault(record, transport_detail);
    }
    *previous = current;
}

fn report_bridge_fault(record: BridgeFaultRecord, transport_detail: Option<&str>) {
    let evidence = record.evidence;
    eprintln!(
        "dmc2-serial-bridge: transition=assert diagnostic={}; evidence: protocol_error={:?}, line_bytes={:?}, previous_sequence={:?}, observed_sequence={:?}, packet_age_ns={:?}, timeout_ns={:?}, previous_quadrature_errors={:?}, observed_quadrature_errors={:?}, operating_system_error={:?}, transport_contract_result={:?}, transport_detail={:?}",
        DiagnosticDisplay(record.code),
        evidence.protocol_error.map(ProtocolError::name),
        evidence.line_bytes,
        evidence.previous_sequence,
        evidence.observed_sequence,
        evidence.packet_age_ns,
        evidence.timeout_ns,
        evidence.previous_quadrature_errors,
        evidence.observed_quadrature_errors,
        evidence.operating_system_error,
        evidence.transport_contract_result,
        transport_detail,
    );
}

fn report_protocol_error(error: ProtocolError, state: &BridgeState) {
    let evidence = state
        .snapshot
        .last_protocol_error
        .filter(|record| record.code == error)
        .map(|record| record.evidence);
    eprintln!(
        "dmc2-serial-bridge: {}; evidence: line_bytes={:?}, previous_sequence={:?}, observed_sequence={:?}, total_protocol_errors={}",
        DiagnosticDisplay(error),
        evidence.and_then(|value| value.line_bytes),
        evidence.and_then(|value| value.previous_sequence),
        evidence.and_then(|value| value.observed_sequence),
        state.snapshot.protocol_errors,
    );
}
