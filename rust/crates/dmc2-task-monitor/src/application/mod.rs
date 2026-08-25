mod audit;
mod cli;
mod diagnostic_state;
mod hal;
mod nml;

use std::ffi::CString;
use std::mem;
use std::thread;
use std::time::Duration;

use crate::diagnostics;
use crate::snapshot::{NativeSnapshot, SNAPSHOT_ABI_VERSION};

use self::cli::arguments;
use self::diagnostic_state::DiagnosticState;
use self::hal::HalPublisher;
use self::nml::{required_nml_error, PollDisposition, StatusChannel};

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
        return audit::run(args.validation_json);
    }

    let nml_file = CString::new(args.nml_file.as_str())
        .map_err(|_| "NML file path contained a NUL byte".to_owned())?;
    let hal = HalPublisher::new(&args.component)?;
    let mut channel = None;
    let mut diagnostic_state = DiagnosticState::new();
    let no_nml_error = required_nml_error("NML_NO_ERROR");
    let invalid_nml_configuration = required_nml_error("NML_INVALID_CONFIGURATION");

    loop {
        if channel.is_none() {
            let (opened, nml_error) = StatusChannel::open(&nml_file, invalid_nml_configuration);
            channel = opened;
            if channel.is_none() || nml_error != no_nml_error {
                channel = None;
                hal.increment_poll_errors();
                hal.publish(
                    NativeSnapshot::safe(),
                    false,
                    true,
                    nml_error,
                    &diagnostics::disconnected(nml_error),
                    &mut diagnostic_state,
                );
                thread::sleep(RECONNECT_PERIOD);
                continue;
            }
        }

        let mut snapshot = NativeSnapshot::safe();
        let (disposition, nml_error) = channel
            .as_mut()
            .expect("NML channel was checked above")
            .poll(&mut snapshot, invalid_nml_configuration, no_nml_error);
        if disposition == PollDisposition::WaitingForFirstStatus {
            thread::sleep(POLL_PERIOD);
            continue;
        }
        let transport_ok = disposition == PollDisposition::Snapshot;
        let snapshot_ok = snapshot.valid_abi();
        if transport_ok && snapshot_ok {
            let report = diagnostics::evaluate(&snapshot);
            hal.publish(
                snapshot,
                true,
                false,
                nml_error,
                &report,
                &mut diagnostic_state,
            );
        } else {
            hal.increment_poll_errors();
            let report = if transport_ok {
                diagnostics::evaluate(&snapshot)
            } else {
                diagnostics::disconnected(nml_error)
            };
            hal.publish(
                NativeSnapshot::safe(),
                false,
                true,
                nml_error,
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
