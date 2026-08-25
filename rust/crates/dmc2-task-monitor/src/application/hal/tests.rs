use std::ffi::{c_char, c_int, c_long, c_void, CStr};
use std::ptr;
use std::string::{String, ToString};
use std::sync::Mutex;
use std::vec::Vec;

use dmc2_hal_sys as hal;

use super::publisher::HalPublisher;
use super::registration::create_hal;

const COMPONENT_ID: c_int = 61;
const PIN_COUNT: usize = 50;

#[repr(align(16))]
struct Arena([u8; 32_768]);

static mut ARENA: Arena = Arena([0; 32_768]);
static mut ARENA_OFFSET: usize = 0;
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
}

impl PinKind {
    const fn function(self) -> &'static str {
        match self {
            Self::Bit => "hal_pin_bit_new",
            Self::S32 => "hal_pin_s32_new",
            Self::U32 => "hal_pin_u32_new",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PinCall {
    name: String,
    kind: PinKind,
    direction: c_int,
}

unsafe fn allocate(size: usize) -> *mut c_void {
    let aligned = unsafe { (ARENA_OFFSET + 15) & !15 };
    let end = aligned.saturating_add(size);
    if end > 32_768 {
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
        ptr::write_bytes(ptr::addr_of_mut!(ARENA.0).cast::<u8>(), 0, 32_768);
    }
    *PLAN.lock().expect("plan lock") = Plan::success();
    *CALLS.lock().expect("call lock") = Calls::new();
    PIN_CALLS.lock().expect("pin-call lock").clear();
}

#[no_mangle]
extern "C" fn hal_init(name: *const c_char) -> c_int {
    assert_eq!(unsafe { CStr::from_ptr(name) }.to_bytes(), b"task-test");
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
    let name = unsafe { CStr::from_ptr(name) }
        .to_str()
        .expect("pin name was UTF-8")
        .to_string();
    let expected_direction = if name == "task-test.clear-latched" {
        hal::hal_pin_dir_t_HAL_IN
    } else {
        hal::hal_pin_dir_t_HAL_OUT
    };
    assert_eq!(direction, expected_direction, "wrong direction for {name}");
    let call = {
        let mut calls = CALLS.lock().expect("call lock");
        let call = calls.pin;
        calls.pin += 1;
        call
    };
    PIN_CALLS.lock().expect("pin-call lock").push(PinCall {
        name,
        kind,
        direction,
    });
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

#[test]
fn every_hal_registration_and_lifecycle_failure_is_exact_and_cleaned_up() {
    for init_result in [-19, 0] {
        reset();
        PLAN.lock().expect("plan lock").init_result = init_result;
        let error = unsafe { create_hal("task-test") }.unwrap_err();
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
    let error = unsafe { create_hal("task-test") }.unwrap_err();
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
        let code = -3_000 - failure_call as c_int;
        PLAN.lock().expect("plan lock").pin_failure = Some((failure_call, code));
        let error = unsafe { create_hal("task-test") }.unwrap_err();
        let calls = PIN_CALLS.lock().expect("pin-call lock");
        let failed = calls.last().expect("failing pin was attempted");
        let suffix = failed
            .name
            .strip_prefix("task-test.")
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
    PLAN.lock().expect("plan lock").ready_result = -4_001;
    let error = unsafe { create_hal("task-test") }.unwrap_err();
    assert_eq!(error, "hal_ready failed: -4001");
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
        plan.exit_result = -4_002;
    }
    let error = unsafe { create_hal("task-test") }.unwrap_err();
    assert_eq!(
        error,
        "hal_malloc for pin-pointer storage failed; hal_exit cleanup failed: -4002"
    );

    reset();
    {
        let publisher = HalPublisher::new("task-test").unwrap();
        assert_eq!(CALLS.lock().expect("call lock").exit, 0);
        assert_eq!(PIN_CALLS.lock().expect("pin-call lock").len(), PIN_COUNT);
        drop(publisher);
    }
    assert_eq!(CALLS.lock().expect("call lock").exit, 1);
}
