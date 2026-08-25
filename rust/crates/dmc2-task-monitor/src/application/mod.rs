mod cli;
mod diagnostic_state;
mod hal;
mod nml;
mod program_validation;

use std::ffi::CString;
use std::mem;
use std::thread;
use std::time::Duration;

use crate::diagnostics;
use crate::snapshot::{NativeSnapshot, SNAPSHOT_ABI_VERSION};

use self::cli::arguments;
use self::diagnostic_state::DiagnosticState;
use self::hal::HalPublisher;
use self::nml::{PollCodes, PollDisposition, StatusChannel};

const POLL_PERIOD: Duration = Duration::from_millis(10);
const RECONNECT_PERIOD: Duration = Duration::from_secs(1);

pub(super) fn run() -> Result<(), String> {
    let args = arguments()?;
    let native_abi = nml::snapshot_abi_version();
    let native_size = nml::snapshot_size();
    if native_abi != SNAPSHOT_ABI_VERSION || native_size != mem::size_of::<NativeSnapshot>() {
        return Err(format!(
            "native snapshot ABI mismatch: C++ version=0x{native_abi:08x} size={native_size}, Rust version=0x{SNAPSHOT_ABI_VERSION:08x} size={}",
            mem::size_of::<NativeSnapshot>()
        ));
    }
    if args.validate {
        return program_validation::run(args.validation_json);
    }

    let nml_file = CString::new(args.nml_file.as_str())
        .map_err(|_| "NML file path contained a NUL byte".to_owned())?;
    let hal = HalPublisher::new(&args.component)?;
    let mut channel = None;
    let mut diagnostic_state = DiagnosticState::new();
    let poll_codes = PollCodes::required();

    loop {
        if channel.is_none() {
            let (opened, transport) = StatusChannel::open(&nml_file, poll_codes);
            channel = opened;
            if channel.is_none() || !transport.healthy(poll_codes) {
                channel = None;
                hal.increment_poll_errors();
                hal.publish(
                    NativeSnapshot::safe(),
                    false,
                    true,
                    transport,
                    &diagnostics::disconnected(transport.nml_error, transport.cms_status),
                    &mut diagnostic_state,
                );
                thread::sleep(RECONNECT_PERIOD);
                continue;
            }
        }

        let mut snapshot = NativeSnapshot::safe();
        let outcome = channel
            .as_mut()
            .expect("NML channel was checked above")
            .poll(&mut snapshot, poll_codes);
        if outcome.disposition == PollDisposition::WaitingForFirstStatus {
            thread::sleep(POLL_PERIOD);
            continue;
        }
        let transport_ok = outcome.disposition == PollDisposition::Snapshot;
        let snapshot_ok = snapshot.valid_abi();
        if transport_ok && snapshot_ok {
            let report = diagnostics::evaluate_with_transport(
                &snapshot,
                outcome.transport.nml_error,
                outcome.transport.cms_status,
            );
            hal.publish(
                snapshot,
                true,
                false,
                outcome.transport,
                &report,
                &mut diagnostic_state,
            );
        } else {
            hal.increment_poll_errors();
            let report = if transport_ok {
                diagnostics::evaluate_with_transport(
                    &snapshot,
                    outcome.transport.nml_error,
                    outcome.transport.cms_status,
                )
            } else {
                diagnostics::disconnected(outcome.transport.nml_error, outcome.transport.cms_status)
            };
            hal.publish(
                NativeSnapshot::safe(),
                false,
                true,
                outcome.transport,
                &report,
                &mut diagnostic_state,
            );
            channel = None;
        }
        thread::sleep(if transport_ok && snapshot_ok {
            POLL_PERIOD
        } else {
            RECONNECT_PERIOD
        });
    }
}
