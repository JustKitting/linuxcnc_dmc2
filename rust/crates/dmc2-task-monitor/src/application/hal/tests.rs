use std::ffi::{c_char, c_int, c_long, c_void, CStr};
use std::ptr;
use std::string::{String, ToString};
use std::sync::Mutex;
use std::vec::Vec;

use dmc2_hal_sys as hal;
use dmc2_linuxcnc_interface::{TASK_INTERP, TASK_MODE, TRAJ_MODE};

use crate::application::diagnostic_state::DiagnosticState;
use crate::application::nml::required_nml_error;
use crate::diagnostics;
use crate::snapshot::{NativeSnapshot, SNAPSHOT_ABI_VERSION};

use super::publisher::HalPublisher;
use super::registration::create_hal;

const COMPONENT_ID: c_int = 61;
const PIN_COUNT: usize = 47;

#[repr(align(16))]
struct Arena([u8; 32_768]);

static mut ARENA: Arena = Arena([0; 32_768]);
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

unsafe fn value<T: Copy>(pointer: *mut T) -> T {
    unsafe { ptr::read_volatile(pointer) }
}

fn code(domain: dmc2_linuxcnc_interface::CodeDomain, name: &str) -> i32 {
    domain
        .codes
        .iter()
        .find(|entry| entry.name == name)
        .expect("required LinuxCNC code exists")
        .code
        .try_into()
        .expect("required LinuxCNC code fits i32")
}

#[test]
fn every_hal_registration_and_lifecycle_failure_is_exact_and_cleaned_up() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
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

#[test]
fn every_hal_output_has_the_exact_status_and_diagnostic_value() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    reset();
    let publisher = HalPublisher::new("task-test").unwrap();
    let pins = publisher.test_pins();
    let mut snapshot = NativeSnapshot::safe();
    snapshot.task.heartbeat = 123_456;
    snapshot.task.mode = code(TASK_MODE, "EMC_TASK_MODE_MANUAL");
    snapshot.task.interp_state = code(TASK_INTERP, "EMC_TASK_INTERP_IDLE");
    snapshot.trajectory.enabled = 1;
    snapshot.trajectory.mode = code(TRAJ_MODE, "EMC_TRAJ_MODE_TELEOP");
    snapshot.io.aux.estop = 0;
    for index in 0..3 {
        snapshot.joints[index].homed = u32::from(index != 1);
        snapshot.joints[index].homing = u32::from(index == 1);
        snapshot.axes[index].stopped = u32::from(index != 2);
    }
    let nml_error = i32::MAX;
    let report = diagnostics::disconnected(nml_error);
    let mut diagnostic_state = DiagnosticState::new();

    publisher.publish(
        snapshot,
        true,
        false,
        nml_error,
        &report,
        &mut diagnostic_state,
    );

    unsafe {
        assert_eq!(value(pins.snapshot_generation), 2);
        assert!(value(pins.connected));
        assert!(!value(pins.fault));
        assert_eq!(value(pins.task_heartbeat), 123_456);
        assert_eq!(value(pins.publications), 1);
        assert_eq!(value(pins.poll_errors), 0);
        assert_eq!(value(pins.nml_error_code), nml_error);
        assert!(!value(pins.nml_error_known));
        assert_eq!(value(pins.linuxcnc_error_active), report.error_active());
        assert_eq!(value(pins.linuxcnc_warning_active), report.warning_active());
        assert_eq!(
            value(pins.unknown_code_active),
            report.unknown_code_active()
        );
        assert_eq!(
            value(pins.active_error_mask_low),
            report.active_error_mask as u32
        );
        assert_eq!(
            value(pins.active_error_mask_high),
            (report.active_error_mask >> 32) as u32
        );
        assert_eq!(
            value(pins.active_warning_mask_low),
            report.active_warning_mask as u32
        );
        assert_eq!(
            value(pins.active_warning_mask_high),
            (report.active_warning_mask >> 32) as u32
        );
        assert_eq!(
            value(pins.latched_error_mask_low),
            diagnostic_state.latched_error_mask as u32
        );
        assert_eq!(
            value(pins.latched_error_mask_high),
            (diagnostic_state.latched_error_mask >> 32) as u32
        );
        assert_eq!(
            value(pins.latched_warning_mask_low),
            diagnostic_state.latched_warning_mask as u32
        );
        assert_eq!(
            value(pins.latched_warning_mask_high),
            (diagnostic_state.latched_warning_mask >> 32) as u32
        );
        assert_eq!(
            value(pins.unknown_domain_mask_low),
            report.unknown_domain_mask as u32
        );
        assert_eq!(
            value(pins.unknown_domain_mask_high),
            (report.unknown_domain_mask >> 32) as u32
        );
        assert_eq!(value(pins.diagnostic_count), report.issues.len() as u32);
        assert_eq!(value(pins.unknown_code_count), report.unknown_code_count());
        assert_eq!(
            value(pins.diagnostic_transitions),
            diagnostic_state.transitions
        );
        assert_eq!(
            value(pins.latest_code_domain),
            diagnostic_state.latest_code_domain
        );
        assert_eq!(
            value(pins.latest_code_low),
            diagnostic_state.latest_code_low
        );
        assert_eq!(
            value(pins.latest_code_high),
            diagnostic_state.latest_code_high
        );
        assert_eq!(
            value(pins.latest_severity),
            diagnostic_state.latest_severity
        );
        assert_eq!(value(pins.latest_action), diagnostic_state.latest_action);
        assert!(!value(pins.clear_latched));
        assert_eq!(value(pins.snapshot_abi_version), SNAPSHOT_ABI_VERSION);
        assert_eq!(
            value(pins.snapshot_struct_size),
            core::mem::size_of::<NativeSnapshot>() as u32
        );
        assert!(value(pins.machine_on));
        assert!(!value(pins.estopped));
        assert!(value(pins.manual_mode));
        assert!(!value(pins.joint_mode));
        assert!(value(pins.teleop_mode));
        assert!(value(pins.interp_idle));
        assert_eq!(
            pins.homed.map(|pointer| value(pointer)),
            [true, false, true]
        );
        assert_eq!(
            pins.homing.map(|pointer| value(pointer)),
            [false, true, false]
        );
        assert_eq!(
            pins.axis_stopped.map(|pointer| value(pointer)),
            [true, true, false]
        );
    }

    publisher.publish(
        snapshot,
        true,
        false,
        required_nml_error("NML_NO_ERROR"),
        &diagnostics::DiagnosticReport::default(),
        &mut diagnostic_state,
    );
    assert_eq!(unsafe { value(pins.snapshot_generation) }, 4);
    assert_eq!(unsafe { value(pins.publications) }, 2);

    unsafe { ptr::write_volatile(pins.publications, u32::MAX) };
    publisher.publish(
        snapshot,
        true,
        false,
        required_nml_error("NML_NO_ERROR"),
        &diagnostics::DiagnosticReport::default(),
        &mut diagnostic_state,
    );
    assert_eq!(unsafe { value(pins.snapshot_generation) }, 6);
    assert_eq!(unsafe { value(pins.publications) }, 3);

    unsafe { ptr::write_volatile(pins.poll_errors, u32::MAX) };
    publisher.increment_poll_errors();
    assert_eq!(unsafe { value(pins.poll_errors) }, 0);
}
