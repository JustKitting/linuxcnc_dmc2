mod cli;
mod hal;
mod serial;

use std::ffi::CString;
use std::thread;
use std::time::{Duration, Instant};

use dmc2_serial_bridge::{AxisCode, BridgeState};

use self::cli::{arguments, DEFAULT_TIMEOUT_MS};
use self::hal::HalPublisher;
use self::serial::{LineAssembler, SerialPort};

const READ_PERIOD: Duration = Duration::from_millis(5);
const RECONNECT_PERIOD: Duration = Duration::from_secs(1);

fn validate() -> Result<(), String> {
    let mut state = BridgeState::new(DEFAULT_TIMEOUT_MS * 1_000_000);
    state
        .accept_line(b"BOOT,P3,MYST1474-001,MONITOR_ONLY", 0)
        .map_err(|error| format!("boot validation failed: {error:?}"))?;
    state
        .accept_line(b"P3,1,20,0,0,0,0,X,X1,0,0,1", 20_000_000)
        .map_err(|error| format!("baseline validation failed: {error:?}"))?;
    state
        .accept_line(b"P3,2,40,1,4,0,1,X,X1,1,0,1", 40_000_000)
        .map_err(|error| format!("detent validation failed: {error:?}"))?;
    if state.snapshot.latest_detent != 1 || state.snapshot.axis != AxisCode::X {
        return Err("validation produced the wrong decoded state".to_owned());
    }
    println!("dmc2-serial-bridge validation: PASS");
    Ok(())
}

pub(super) fn run() -> Result<(), String> {
    let args = arguments()?;
    if args.validate {
        return validate();
    }
    let timeout_ns = args
        .timeout_ms
        .checked_mul(1_000_000)
        .ok_or_else(|| "packet timeout overflowed nanoseconds".to_owned())?;
    let port = CString::new(args.port.as_str())
        .map_err(|_| "serial path contained a NUL byte".to_owned())?;
    let hal = HalPublisher::new(&args.component)?;
    let epoch = Instant::now();
    let mut state = BridgeState::new(timeout_ns);
    hal.publish(state.snapshot, -1.0);

    loop {
        let Some(serial) = SerialPort::open(&port, args.baud) else {
            state.note_serial_fault();
            hal.publish(state.snapshot, -1.0);
            thread::sleep(RECONNECT_PERIOD);
            continue;
        };

        let mut assembler = LineAssembler::new();
        let mut buffer = [0_u8; 256];
        loop {
            let count = match serial.read(&mut buffer) {
                Ok(value) => value,
                Err(()) => {
                    state.note_serial_fault();
                    hal.publish(state.snapshot, -1.0);
                    break;
                }
            };
            let now_ns = epoch.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            let mut published_line = false;
            for &byte in &buffer[..count] {
                published_line |= assembler
                    .consume(byte, &mut state, now_ns)
                    .requires_publish();
            }
            let timed_out = state.check_timeout(now_ns);
            if published_line || timed_out || count == 0 {
                hal.publish(state.snapshot, state.packet_age_ms(now_ns));
            }
            thread::sleep(READ_PERIOD);
        }
    }
}
