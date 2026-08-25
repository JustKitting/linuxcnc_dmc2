use std::env;
use std::ffi::{c_char, c_int, c_uint, CString};
use std::mem;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use dmc2_hal_sys as hal;
use dmc2_serial_bridge::{AxisCode, BridgeState, MultiplierCode, Snapshot};

const DEFAULT_COMPONENT: &str = "dmc2-pendant";
const DEFAULT_PORT: &str = "/dev/ttyUSB0";
const DEFAULT_BAUD: u32 = 115_200;
const DEFAULT_TIMEOUT_MS: u64 = 100;
const READ_PERIOD: Duration = Duration::from_millis(5);
const RECONNECT_PERIOD: Duration = Duration::from_secs(1);

unsafe extern "C" {
    fn dmc2_serial_open(path: *const c_char, baud: c_uint) -> c_int;
    fn dmc2_serial_read(fd: c_int, buffer: *mut u8, capacity: usize) -> c_int;
    fn dmc2_serial_close(fd: c_int);
}

#[derive(Debug)]
struct Arguments {
    component: String,
    port: String,
    baud: u32,
    timeout_ms: u64,
    validate: bool,
}

fn arguments() -> Result<Arguments, String> {
    let mut result = Arguments {
        component: DEFAULT_COMPONENT.to_owned(),
        port: DEFAULT_PORT.to_owned(),
        baud: DEFAULT_BAUD,
        timeout_ms: DEFAULT_TIMEOUT_MS,
        validate: false,
    };
    let mut items = env::args().skip(1);
    while let Some(argument) = items.next() {
        let value = |items: &mut std::iter::Skip<std::env::Args>| {
            items
                .next()
                .ok_or_else(|| format!("{argument} requires a value"))
        };
        match argument.as_str() {
            "--component" => result.component = value(&mut items)?,
            "--port" => result.port = value(&mut items)?,
            "--baud" => {
                result.baud = value(&mut items)?
                    .parse()
                    .map_err(|_| "--baud must be an unsigned integer".to_owned())?;
            }
            "--packet-timeout-ms" => {
                result.timeout_ms = value(&mut items)?
                    .parse()
                    .map_err(|_| "--packet-timeout-ms must be an unsigned integer".to_owned())?;
            }
            "--validate" => result.validate = true,
            "--help" | "-h" => {
                println!(
                    "Usage: dmc2-serial-bridge [--component NAME] [--port PATH] \
                     [--baud 115200] [--packet-timeout-ms N] [--validate]"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    if result.timeout_ms == 0 {
        return Err("--packet-timeout-ms must be positive".to_owned());
    }
    if result.baud != DEFAULT_BAUD {
        return Err("this audited bridge accepts exactly 115200 baud".to_owned());
    }
    Ok(result)
}

struct HalPins {
    snapshot_generation: *mut hal::hal_u32_t,
    connected: *mut hal::hal_bit_t,
    serial_fault: *mut hal::hal_bit_t,
    quadrature_fault: *mut hal::hal_bit_t,
    link_healthy: *mut hal::hal_bit_t,
    heartbeat: *mut hal::hal_bit_t,
    estop_pressed: *mut hal::hal_bit_t,
    deadman_held: *mut hal::hal_bit_t,
    selector_valid: *mut hal::hal_bit_t,
    axis: [*mut hal::hal_bit_t; 7],
    multiplier: [*mut hal::hal_bit_t; 5],
    axis_code: *mut hal::hal_s32_t,
    multiplier_code: *mut hal::hal_s32_t,
    latest_detent: *mut hal::hal_s32_t,
    detent_count: *mut hal::hal_s32_t,
    transition_count: *mut hal::hal_s32_t,
    quadrature_errors: *mut hal::hal_u32_t,
    sequence: *mut hal::hal_u32_t,
    milliseconds: *mut hal::hal_u32_t,
    protocol_errors: *mut hal::hal_u32_t,
    dropped_packets: *mut hal::hal_u32_t,
    timeouts: *mut hal::hal_u32_t,
    packet_age_ms: *mut hal::real_t,
}

impl HalPins {
    const fn empty() -> Self {
        Self {
            snapshot_generation: ptr::null_mut(),
            connected: ptr::null_mut(),
            serial_fault: ptr::null_mut(),
            quadrature_fault: ptr::null_mut(),
            link_healthy: ptr::null_mut(),
            heartbeat: ptr::null_mut(),
            estop_pressed: ptr::null_mut(),
            deadman_held: ptr::null_mut(),
            selector_valid: ptr::null_mut(),
            axis: [ptr::null_mut(); 7],
            multiplier: [ptr::null_mut(); 5],
            axis_code: ptr::null_mut(),
            multiplier_code: ptr::null_mut(),
            latest_detent: ptr::null_mut(),
            detent_count: ptr::null_mut(),
            transition_count: ptr::null_mut(),
            quadrature_errors: ptr::null_mut(),
            sequence: ptr::null_mut(),
            milliseconds: ptr::null_mut(),
            protocol_errors: ptr::null_mut(),
            dropped_packets: ptr::null_mut(),
            timeouts: ptr::null_mut(),
            packet_age_ms: ptr::null_mut(),
        }
    }
}

fn pin_name(component: &str, suffix: &str) -> Result<CString, String> {
    CString::new(format!("{component}.{suffix}"))
        .map_err(|_| "HAL pin name contained a NUL byte".to_owned())
}

unsafe fn bit_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::hal_bit_t,
    component_id: c_int,
) -> Result<(), String> {
    let name = pin_name(component, suffix)?;
    let result = unsafe {
        hal::hal_pin_bit_new(
            name.as_ptr(),
            hal::hal_pin_dir_t_HAL_OUT,
            pointer,
            component_id,
        )
    };
    (result == 0)
        .then_some(())
        .ok_or_else(|| format!("hal_pin_bit_new({suffix}) failed: {result}"))
}

unsafe fn s32_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::hal_s32_t,
    component_id: c_int,
) -> Result<(), String> {
    let name = pin_name(component, suffix)?;
    let result = unsafe {
        hal::hal_pin_s32_new(
            name.as_ptr(),
            hal::hal_pin_dir_t_HAL_OUT,
            pointer,
            component_id,
        )
    };
    (result == 0)
        .then_some(())
        .ok_or_else(|| format!("hal_pin_s32_new({suffix}) failed: {result}"))
}

unsafe fn u32_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::hal_u32_t,
    component_id: c_int,
) -> Result<(), String> {
    let name = pin_name(component, suffix)?;
    let result = unsafe {
        hal::hal_pin_u32_new(
            name.as_ptr(),
            hal::hal_pin_dir_t_HAL_OUT,
            pointer,
            component_id,
        )
    };
    (result == 0)
        .then_some(())
        .ok_or_else(|| format!("hal_pin_u32_new({suffix}) failed: {result}"))
}

unsafe fn float_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::real_t,
    component_id: c_int,
) -> Result<(), String> {
    let name = pin_name(component, suffix)?;
    let result = unsafe {
        hal::hal_pin_float_new(
            name.as_ptr(),
            hal::hal_pin_dir_t_HAL_OUT,
            pointer,
            component_id,
        )
    };
    (result == 0)
        .then_some(())
        .ok_or_else(|| format!("hal_pin_float_new({suffix}) failed: {result}"))
}

unsafe fn create_hal(component: &str) -> Result<(c_int, *mut HalPins), String> {
    let component_name = CString::new(component)
        .map_err(|_| "HAL component name contained a NUL byte".to_owned())?;
    let component_id = unsafe { hal::hal_init(component_name.as_ptr()) };
    if component_id < 0 {
        return Err(format!("hal_init failed: {component_id}"));
    }

    let result = (|| {
        let pins_pointer =
            unsafe { hal::hal_malloc(mem::size_of::<HalPins>() as _) } as *mut HalPins;
        if pins_pointer.is_null() {
            return Err("hal_malloc for pin-pointer storage failed".to_owned());
        }
        unsafe { ptr::write(pins_pointer, HalPins::empty()) };
        let pins = unsafe { &mut *pins_pointer };
        unsafe {
            u32_pin(
                component,
                "snapshot-generation",
                &mut pins.snapshot_generation,
                component_id,
            )?;
            bit_pin(component, "connected", &mut pins.connected, component_id)?;
            bit_pin(
                component,
                "serial-fault",
                &mut pins.serial_fault,
                component_id,
            )?;
            bit_pin(
                component,
                "quadrature-fault",
                &mut pins.quadrature_fault,
                component_id,
            )?;
            bit_pin(
                component,
                "link-healthy",
                &mut pins.link_healthy,
                component_id,
            )?;
            bit_pin(component, "heartbeat", &mut pins.heartbeat, component_id)?;
            bit_pin(
                component,
                "estop-pressed",
                &mut pins.estop_pressed,
                component_id,
            )?;
            bit_pin(
                component,
                "deadman-held",
                &mut pins.deadman_held,
                component_id,
            )?;
            bit_pin(
                component,
                "selector-valid",
                &mut pins.selector_valid,
                component_id,
            )?;
            for (index, suffix) in [
                "axis-x",
                "axis-y",
                "axis-z",
                "axis-4",
                "axis-5",
                "axis-off",
                "axis-invalid",
            ]
            .iter()
            .enumerate()
            {
                bit_pin(component, suffix, &mut pins.axis[index], component_id)?;
            }
            for (index, suffix) in [
                "multiplier-x1",
                "multiplier-x10",
                "multiplier-x100",
                "multiplier-off",
                "multiplier-invalid",
            ]
            .iter()
            .enumerate()
            {
                bit_pin(component, suffix, &mut pins.multiplier[index], component_id)?;
            }
            s32_pin(component, "axis-code", &mut pins.axis_code, component_id)?;
            s32_pin(
                component,
                "multiplier-code",
                &mut pins.multiplier_code,
                component_id,
            )?;
            s32_pin(
                component,
                "latest-detent",
                &mut pins.latest_detent,
                component_id,
            )?;
            s32_pin(
                component,
                "detent-count",
                &mut pins.detent_count,
                component_id,
            )?;
            s32_pin(
                component,
                "transition-count",
                &mut pins.transition_count,
                component_id,
            )?;
            u32_pin(
                component,
                "quadrature-errors",
                &mut pins.quadrature_errors,
                component_id,
            )?;
            u32_pin(component, "sequence", &mut pins.sequence, component_id)?;
            u32_pin(
                component,
                "milliseconds",
                &mut pins.milliseconds,
                component_id,
            )?;
            u32_pin(
                component,
                "protocol-errors",
                &mut pins.protocol_errors,
                component_id,
            )?;
            u32_pin(
                component,
                "dropped-packets",
                &mut pins.dropped_packets,
                component_id,
            )?;
            u32_pin(component, "timeouts", &mut pins.timeouts, component_id)?;
            float_pin(
                component,
                "packet-age-ms",
                &mut pins.packet_age_ms,
                component_id,
            )?;
        }
        let ready = unsafe { hal::hal_ready(component_id) };
        if ready != 0 {
            return Err(format!("hal_ready failed: {ready}"));
        }
        Ok(pins_pointer)
    })();
    match result {
        Ok(pins) => Ok((component_id, pins)),
        Err(error) => {
            unsafe { hal::hal_exit(component_id) };
            Err(error)
        }
    }
}

unsafe fn write<T: Copy>(pointer: *mut T, value: T) {
    unsafe { ptr::write_volatile(pointer, value) };
}

unsafe fn publish(pins: &HalPins, snapshot: Snapshot, packet_age_ms: f64) {
    let generation = (snapshot.sequence & 0x7fff_ffff) << 1;
    let generation_pin = unsafe { &*(pins.snapshot_generation.cast::<AtomicU32>()) };
    unsafe {
        generation_pin.store(generation | 1, Ordering::SeqCst);
        write(pins.connected, snapshot.connected);
        write(pins.serial_fault, snapshot.serial_fault);
        write(pins.quadrature_fault, snapshot.quadrature_fault);
        write(pins.link_healthy, snapshot.link_healthy);
        write(pins.heartbeat, snapshot.heartbeat);
        write(pins.estop_pressed, snapshot.estop_pressed);
        write(pins.deadman_held, snapshot.deadman_held);
        write(pins.selector_valid, snapshot.selector_valid);
        for (pointer, active) in pins.axis.iter().zip([
            snapshot.axis == AxisCode::X,
            snapshot.axis == AxisCode::Y,
            snapshot.axis == AxisCode::Z,
            snapshot.axis == AxisCode::Axis4,
            snapshot.axis == AxisCode::Axis5,
            snapshot.axis == AxisCode::Off,
            snapshot.axis == AxisCode::Invalid,
        ]) {
            write(*pointer, active);
        }
        for (pointer, active) in pins.multiplier.iter().zip([
            snapshot.multiplier == MultiplierCode::X1,
            snapshot.multiplier == MultiplierCode::X10,
            snapshot.multiplier == MultiplierCode::X100,
            snapshot.multiplier == MultiplierCode::Off,
            snapshot.multiplier == MultiplierCode::Invalid,
        ]) {
            write(*pointer, active);
        }
        write(pins.axis_code, snapshot.axis as i32);
        write(pins.multiplier_code, snapshot.multiplier as i32);
        write(pins.latest_detent, snapshot.latest_detent);
        write(pins.detent_count, snapshot.detent_count);
        write(pins.transition_count, snapshot.transition_count);
        write(pins.quadrature_errors, snapshot.quadrature_errors);
        write(pins.sequence, snapshot.sequence);
        write(pins.milliseconds, snapshot.milliseconds);
        write(pins.protocol_errors, snapshot.protocol_errors);
        write(pins.dropped_packets, snapshot.dropped_packets);
        write(pins.timeouts, snapshot.timeouts);
        write(pins.packet_age_ms, packet_age_ms);
        generation_pin.store(generation, Ordering::SeqCst);
    }
}

struct LineAssembler {
    bytes: [u8; dmc2_serial_bridge::MAX_SERIAL_LINE_BYTES],
    length: usize,
    overlong: bool,
}

impl LineAssembler {
    const fn new() -> Self {
        Self {
            bytes: [0; dmc2_serial_bridge::MAX_SERIAL_LINE_BYTES],
            length: 0,
            overlong: false,
        }
    }

    fn consume(&mut self, byte: u8, state: &mut BridgeState, now_ns: u64) -> bool {
        if byte == b'\r' {
            return false;
        }
        if byte != b'\n' {
            if self.length < self.bytes.len() {
                self.bytes[self.length] = byte;
                self.length += 1;
            } else {
                self.overlong = true;
            }
            return false;
        }

        if self.overlong {
            state.note_protocol_error();
        } else if self.length > 0 {
            let _ = state.accept_line(&self.bytes[..self.length], now_ns);
        }
        self.length = 0;
        self.overlong = false;
        true
    }
}

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

fn run() -> Result<(), String> {
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
    let (_component_id, pins_pointer) = unsafe { create_hal(&args.component)? };
    let pins = unsafe { &*pins_pointer };
    let epoch = Instant::now();
    let mut state = BridgeState::new(timeout_ns);
    unsafe { publish(&pins, state.snapshot, -1.0) };

    loop {
        let fd = unsafe { dmc2_serial_open(port.as_ptr(), args.baud) };
        if fd < 0 {
            state.note_serial_fault();
            unsafe { publish(&pins, state.snapshot, -1.0) };
            thread::sleep(RECONNECT_PERIOD);
            continue;
        }

        let mut assembler = LineAssembler::new();
        let mut buffer = [0_u8; 256];
        loop {
            let count = unsafe { dmc2_serial_read(fd, buffer.as_mut_ptr(), buffer.len()) };
            let now_ns = epoch.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            if count < 0 {
                unsafe { dmc2_serial_close(fd) };
                state.note_serial_fault();
                unsafe { publish(&pins, state.snapshot, -1.0) };
                break;
            }

            let mut published_line = false;
            for &byte in &buffer[..count as usize] {
                published_line |= assembler.consume(byte, &mut state, now_ns);
            }
            let timed_out = state.check_timeout(now_ns);
            if published_line || timed_out || count == 0 {
                unsafe { publish(&pins, state.snapshot, state.packet_age_ms(now_ns)) };
            }
            thread::sleep(READ_PERIOD);
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("dmc2-serial-bridge: {error}");
        std::process::exit(1);
    }
}
