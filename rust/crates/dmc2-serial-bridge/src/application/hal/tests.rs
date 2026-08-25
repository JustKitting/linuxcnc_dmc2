use std::ffi::{c_char, c_int, c_long, c_void, CStr};
use std::ptr;
use std::string::{String, ToString};
use std::sync::Mutex;
use std::vec::Vec;

use dmc2_hal_sys as hal;
use dmc2_serial_bridge::{AxisCode, MultiplierCode, Snapshot};

use super::publisher::HalPublisher;
use super::registration::create_hal;

const COMPONENT_ID: c_int = 51;
const PIN_COUNT: usize = 33;

#[repr(align(16))]
struct Arena([u8; 16_384]);

static mut ARENA: Arena = Arena([0; 16_384]);
static mut ARENA_OFFSET: usize = 0;
static TEST_LOCK: Mutex<()> = Mutex::new(());
static PLAN: Mutex<Plan> = Mutex::new(Plan::success());
static CALLS: Mutex<Calls> = Mutex::new(Calls::new());
static PIN_CALLS: Mutex<Vec<PinCall>> = Mutex::new(Vec::new());

#[derive(Clone, Copy)]
struct Plan {
    init_result: c_int,
    malloc_fails: bool,
    pin_failure: Option<(usize, c_int)>,
    ready_result: c_int,
    exit_result: c_int,
}

impl Plan {
    const fn success() -> Self {
        Self {
            init_result: COMPONENT_ID,
            malloc_fails: false,
            pin_failure: None,
            ready_result: 0,
            exit_result: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Calls {
    init: usize,
    malloc: usize,
    pin: usize,
    ready: usize,
    exit: usize,
}

impl Calls {
    const fn new() -> Self {
        Self {
            init: 0,
            malloc: 0,
            pin: 0,
            ready: 0,
            exit: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PinKind {
    Bit,
    S32,
    U32,
    Float,
}

impl PinKind {
    const fn function(self) -> &'static str {
        match self {
            Self::Bit => "hal_pin_bit_new",
            Self::S32 => "hal_pin_s32_new",
            Self::U32 => "hal_pin_u32_new",
            Self::Float => "hal_pin_float_new",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PinCall {
    name: String,
    kind: PinKind,
}

unsafe fn allocate(size: usize) -> *mut c_void {
    let aligned = unsafe { (ARENA_OFFSET + 15) & !15 };
    let end = aligned.saturating_add(size);
    if end > 16_384 {
        return ptr::null_mut();
    }
    unsafe {
        ARENA_OFFSET = end;
        ptr::addr_of_mut!(ARENA.0)
            .cast::<u8>()
            .add(aligned)
            .cast::<c_void>()
    }
}

fn reset() {
    unsafe {
        ARENA_OFFSET = 0;
        ptr::write_bytes(ptr::addr_of_mut!(ARENA.0).cast::<u8>(), 0, 16_384);
    }
    *PLAN.lock().expect("plan lock") = Plan::success();
    *CALLS.lock().expect("call lock") = Calls::new();
    PIN_CALLS.lock().expect("pin-call lock").clear();
}

#[no_mangle]
extern "C" fn hal_init(name: *const c_char) -> c_int {
    assert_eq!(unsafe { CStr::from_ptr(name) }.to_bytes(), b"serial-test");
    CALLS.lock().expect("call lock").init += 1;
    PLAN.lock().expect("plan lock").init_result
}

#[no_mangle]
extern "C" fn hal_exit(component_id: c_int) -> c_int {
    assert_eq!(component_id, COMPONENT_ID);
    CALLS.lock().expect("call lock").exit += 1;
    PLAN.lock().expect("plan lock").exit_result
}

#[no_mangle]
extern "C" fn hal_ready(component_id: c_int) -> c_int {
    assert_eq!(component_id, COMPONENT_ID);
    CALLS.lock().expect("call lock").ready += 1;
    PLAN.lock().expect("plan lock").ready_result
}

#[no_mangle]
extern "C" fn hal_malloc(size: c_long) -> *mut c_void {
    CALLS.lock().expect("call lock").malloc += 1;
    if size <= 0 || PLAN.lock().expect("plan lock").malloc_fails {
        ptr::null_mut()
    } else {
        unsafe { allocate(size as usize) }
    }
}

unsafe fn register<T>(
    name: *const c_char,
    direction: c_int,
    pointer: *mut *mut T,
    component_id: c_int,
    kind: PinKind,
) -> c_int {
    assert_eq!(component_id, COMPONENT_ID);
    assert_eq!(direction, hal::hal_pin_dir_t_HAL_OUT);
    let name = unsafe { CStr::from_ptr(name) }
        .to_str()
        .expect("pin name was UTF-8")
        .to_string();
    let call = {
        let mut calls = CALLS.lock().expect("call lock");
        let call = calls.pin;
        calls.pin += 1;
        call
    };
    PIN_CALLS
        .lock()
        .expect("pin-call lock")
        .push(PinCall { name, kind });
    if let Some((failure_call, error)) = PLAN.lock().expect("plan lock").pin_failure {
        if failure_call == call {
            return error;
        }
    }
    let data = unsafe { allocate(core::mem::size_of::<T>()) }.cast::<T>();
    if data.is_null() {
        return -12;
    }
    unsafe {
        ptr::write_bytes(data, 0, 1);
        ptr::write(pointer, data);
    }
    0
}

#[no_mangle]
extern "C" fn hal_pin_bit_new(
    name: *const c_char,
    direction: c_int,
    pointer: *mut *mut bool,
    component_id: c_int,
) -> c_int {
    unsafe { register(name, direction, pointer, component_id, PinKind::Bit) }
}

#[no_mangle]
extern "C" fn hal_pin_s32_new(
    name: *const c_char,
    direction: c_int,
    pointer: *mut *mut i32,
    component_id: c_int,
) -> c_int {
    unsafe { register(name, direction, pointer, component_id, PinKind::S32) }
}

#[no_mangle]
extern "C" fn hal_pin_u32_new(
    name: *const c_char,
    direction: c_int,
    pointer: *mut *mut u32,
    component_id: c_int,
) -> c_int {
    unsafe { register(name, direction, pointer, component_id, PinKind::U32) }
}

#[no_mangle]
extern "C" fn hal_pin_float_new(
    name: *const c_char,
    direction: c_int,
    pointer: *mut *mut f64,
    component_id: c_int,
) -> c_int {
    unsafe { register(name, direction, pointer, component_id, PinKind::Float) }
}

unsafe fn value<T: Copy>(pointer: *mut T) -> T {
    unsafe { ptr::read_volatile(pointer) }
}

#[test]
fn every_hal_registration_and_lifecycle_failure_is_exact_and_cleaned_up() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    for init_result in [-19, 0] {
        reset();
        PLAN.lock().expect("plan lock").init_result = init_result;
        let error = unsafe { create_hal("serial-test") }.unwrap_err();
        assert_eq!(error, format!("hal_init failed: {init_result}"));
        assert_eq!(
            *CALLS.lock().expect("call lock"),
            Calls {
                init: 1,
                ..Calls::new()
            }
        );
    }

    reset();
    let error = unsafe { create_hal("bad\0component") }.unwrap_err();
    assert_eq!(error, "HAL component name contained a NUL byte");
    assert_eq!(*CALLS.lock().expect("call lock"), Calls::new());

    reset();
    PLAN.lock().expect("plan lock").malloc_fails = true;
    let error = unsafe { create_hal("serial-test") }.unwrap_err();
    assert_eq!(error, "hal_malloc for pin-pointer storage failed");
    assert_eq!(
        *CALLS.lock().expect("call lock"),
        Calls {
            init: 1,
            malloc: 1,
            pin: 0,
            ready: 0,
            exit: 1,
        }
    );

    for failure_call in 0..PIN_COUNT {
        reset();
        let code = -1_000 - failure_call as c_int;
        PLAN.lock().expect("plan lock").pin_failure = Some((failure_call, code));
        let error = unsafe { create_hal("serial-test") }.unwrap_err();
        let calls = PIN_CALLS.lock().expect("pin-call lock");
        let failed = calls.last().expect("failing pin was attempted");
        let suffix = failed
            .name
            .strip_prefix("serial-test.")
            .expect("component prefix was exact");
        assert_eq!(
            error,
            format!("{}({suffix}) failed: {code}", failed.kind.function())
        );
        assert_eq!(calls.len(), failure_call + 1);
        let hal_calls = *CALLS.lock().expect("call lock");
        assert_eq!(hal_calls.init, 1);
        assert_eq!(hal_calls.malloc, 1);
        assert_eq!(hal_calls.pin, failure_call + 1);
        assert_eq!(hal_calls.ready, 0);
        assert_eq!(hal_calls.exit, 1);
    }

    reset();
    PLAN.lock().expect("plan lock").ready_result = -2_001;
    let error = unsafe { create_hal("serial-test") }.unwrap_err();
    assert_eq!(error, "hal_ready failed: -2001");
    assert_eq!(
        *CALLS.lock().expect("call lock"),
        Calls {
            init: 1,
            malloc: 1,
            pin: PIN_COUNT,
            ready: 1,
            exit: 1,
        }
    );

    reset();
    {
        let mut plan = PLAN.lock().expect("plan lock");
        plan.malloc_fails = true;
        plan.exit_result = -2_002;
    }
    let error = unsafe { create_hal("serial-test") }.unwrap_err();
    assert_eq!(
        error,
        "hal_malloc for pin-pointer storage failed; hal_exit cleanup failed: -2002"
    );

    reset();
    {
        let publisher = HalPublisher::new("serial-test").unwrap();
        assert_eq!(CALLS.lock().expect("call lock").exit, 0);
        drop(publisher);
    }
    assert_eq!(CALLS.lock().expect("call lock").exit, 1);
}

#[test]
fn every_hal_output_has_the_exact_snapshot_value_and_local_generation() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    reset();
    let publisher = HalPublisher::new("serial-test").unwrap();
    let snapshot = Snapshot {
        connected: true,
        serial_fault: false,
        quadrature_fault: true,
        link_healthy: false,
        heartbeat: true,
        estop_pressed: false,
        deadman_held: true,
        selector_valid: true,
        axis: AxisCode::Y,
        multiplier: MultiplierCode::X100,
        latest_detent: -1,
        detent_count: -123_456,
        transition_count: 654_321,
        quadrature_errors: 7,
        sequence: 99,
        milliseconds: 1_234_567,
        protocol_errors: 8,
        dropped_packets: 9,
        timeouts: 10,
    };

    publisher.publish(snapshot, 12.5);
    let pins = publisher.test_pins();
    unsafe {
        assert_eq!(value(pins.snapshot_generation), 2);
        assert!(value(pins.connected));
        assert!(!value(pins.serial_fault));
        assert!(value(pins.quadrature_fault));
        assert!(!value(pins.link_healthy));
        assert!(value(pins.heartbeat));
        assert!(!value(pins.estop_pressed));
        assert!(value(pins.deadman_held));
        assert!(value(pins.selector_valid));
        assert_eq!(
            pins.axis.map(|pointer| value(pointer)),
            [false, true, false, false, false, false, false]
        );
        assert_eq!(
            pins.multiplier.map(|pointer| value(pointer)),
            [false, false, true, false, false]
        );
        assert_eq!(value(pins.axis_code), AxisCode::Y as i32);
        assert_eq!(value(pins.multiplier_code), MultiplierCode::X100 as i32);
        assert_eq!(value(pins.latest_detent), -1);
        assert_eq!(value(pins.detent_count), -123_456);
        assert_eq!(value(pins.transition_count), 654_321);
        assert_eq!(value(pins.quadrature_errors), 7);
        assert_eq!(value(pins.sequence), 99);
        assert_eq!(value(pins.milliseconds), 1_234_567);
        assert_eq!(value(pins.protocol_errors), 8);
        assert_eq!(value(pins.dropped_packets), 9);
        assert_eq!(value(pins.timeouts), 10);
        assert_eq!(value(pins.packet_age_ms), 12.5);
    }

    publisher.publish(snapshot, 13.5);
    assert_eq!(unsafe { value(pins.snapshot_generation) }, 4);
    publisher.publish(
        Snapshot {
            sequence: 0,
            ..snapshot
        },
        14.5,
    );
    assert_eq!(unsafe { value(pins.snapshot_generation) }, 6);
    assert_eq!(unsafe { value(pins.sequence) }, 0);
}
