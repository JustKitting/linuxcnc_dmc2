//! Mock-HAL integration tests for the exported realtime component.

use super::*;
use dmc2_hal_sys as hal;
use std::string::{String, ToString};
use std::sync::Mutex;
use std::vec::Vec;

mod schema;
mod transport;

#[repr(align(16))]
struct Arena([u8; 131_072]);

static mut ARENA: Arena = Arena([0; 131_072]);
static mut ARENA_OFFSET: usize = 0;
static TEST_LOCK: Mutex<()> = Mutex::new(());
static PINS: Mutex<Vec<PinRecord>> = Mutex::new(Vec::new());
static FUNCTION: Mutex<Option<(usize, usize)>> = Mutex::new(None);
static FAILURE_PLAN: Mutex<FailurePlan> = Mutex::new(FailurePlan::success());
static HAL_CALLS: Mutex<HalCalls> = Mutex::new(HalCalls::new());
static RTAPI_MESSAGES: Mutex<Vec<(hal::msg_level_t, String)>> = Mutex::new(Vec::new());

#[derive(Clone, Copy, Debug)]
struct FailurePlan {
    init_result: c_int,
    malloc_failure: Option<usize>,
    pin_failure: Option<(usize, c_int)>,
    export_result: c_int,
    ready_result: c_int,
    exit_result: c_int,
}

impl FailurePlan {
    const fn success() -> Self {
        Self {
            init_result: 41,
            malloc_failure: None,
            pin_failure: None,
            export_result: 0,
            ready_result: 0,
            exit_result: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HalCalls {
    init: usize,
    exit: usize,
    ready: usize,
    malloc: usize,
    pin: usize,
    export: usize,
}

impl HalCalls {
    const fn new() -> Self {
        Self {
            init: 0,
            exit: 0,
            ready: 0,
            malloc: 0,
            pin: 0,
            export: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum PinKind {
    Bit,
    S32,
    U32,
    Float,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PinRecord {
    name: String,
    address: usize,
    kind: PinKind,
    direction: c_int,
}

unsafe fn allocate(size: usize) -> *mut c_void {
    let aligned = unsafe { (ARENA_OFFSET + 15) & !15 };
    let end = aligned.saturating_add(size);
    if end > 131_072 {
        return ptr::null_mut();
    }
    unsafe {
        ARENA_OFFSET = end;
        (ptr::addr_of_mut!(ARENA.0) as *mut u8)
            .add(aligned)
            .cast::<c_void>()
    }
}

unsafe fn register<T>(
    name: *const c_char,
    direction: c_int,
    pointer: *mut *mut T,
    kind: PinKind,
) -> c_int {
    let data = unsafe { allocate(mem::size_of::<T>()) }.cast::<T>();
    if data.is_null() {
        return ENOMEM;
    }
    unsafe {
        ptr::write_bytes(data, 0, 1);
        ptr::write(pointer, data);
    }
    let name = unsafe { std::ffi::CStr::from_ptr(name) }
        .to_str()
        .expect("mock HAL pin name was UTF-8")
        .to_string();
    PINS.lock()
        .expect("mock HAL registry lock")
        .push(PinRecord {
            name,
            address: data as usize,
            kind,
            direction,
        });
    0
}

#[no_mangle]
extern "C" fn hal_init(name: *const c_char) -> c_int {
    let name = unsafe { std::ffi::CStr::from_ptr(name) }
        .to_str()
        .expect("component name was UTF-8");
    assert_eq!(name, "dmc2_rt");
    HAL_CALLS.lock().expect("HAL call lock").init += 1;
    FAILURE_PLAN.lock().expect("failure plan lock").init_result
}

#[no_mangle]
extern "C" fn hal_exit(component_id: c_int) -> c_int {
    assert_eq!(component_id, 41);
    HAL_CALLS.lock().expect("HAL call lock").exit += 1;
    FAILURE_PLAN.lock().expect("failure plan lock").exit_result
}

#[no_mangle]
extern "C" fn hal_ready(component_id: c_int) -> c_int {
    assert_eq!(component_id, 41);
    HAL_CALLS.lock().expect("HAL call lock").ready += 1;
    FAILURE_PLAN.lock().expect("failure plan lock").ready_result
}

#[no_mangle]
extern "C" fn rtapi_print_msg(level: hal::msg_level_t, format: *const c_char) {
    let message = unsafe { std::ffi::CStr::from_ptr(format) }
        .to_str()
        .expect("RTAPI message was UTF-8")
        .to_string();
    RTAPI_MESSAGES
        .lock()
        .expect("RTAPI message lock")
        .push((level, message));
}

#[no_mangle]
extern "C" fn hal_malloc(size: c_long) -> *mut c_void {
    let call = {
        let mut calls = HAL_CALLS.lock().expect("HAL call lock");
        let call = calls.malloc;
        calls.malloc += 1;
        call
    };
    if FAILURE_PLAN
        .lock()
        .expect("failure plan lock")
        .malloc_failure
        == Some(call)
    {
        return ptr::null_mut();
    }
    if size <= 0 {
        return ptr::null_mut();
    }
    unsafe { allocate(size as usize) }
}

fn injected_pin_failure() -> Option<c_int> {
    let call = {
        let mut calls = HAL_CALLS.lock().expect("HAL call lock");
        let call = calls.pin;
        calls.pin += 1;
        call
    };
    FAILURE_PLAN
        .lock()
        .expect("failure plan lock")
        .pin_failure
        .filter(|(failure_call, _)| *failure_call == call)
        .map(|(_, error)| error)
}

#[no_mangle]
extern "C" fn hal_pin_bit_new(
    name: *const c_char,
    direction: c_int,
    pointer: *mut *mut bool,
    _component_id: c_int,
) -> c_int {
    if let Some(error) = injected_pin_failure() {
        return error;
    }
    unsafe { register(name, direction, pointer, PinKind::Bit) }
}

#[no_mangle]
extern "C" fn hal_pin_s32_new(
    name: *const c_char,
    direction: c_int,
    pointer: *mut *mut i32,
    _component_id: c_int,
) -> c_int {
    if let Some(error) = injected_pin_failure() {
        return error;
    }
    unsafe { register(name, direction, pointer, PinKind::S32) }
}

#[no_mangle]
extern "C" fn hal_pin_u32_new(
    name: *const c_char,
    direction: c_int,
    pointer: *mut *mut u32,
    _component_id: c_int,
) -> c_int {
    if let Some(error) = injected_pin_failure() {
        return error;
    }
    unsafe { register(name, direction, pointer, PinKind::U32) }
}

#[no_mangle]
extern "C" fn hal_pin_float_new(
    name: *const c_char,
    direction: c_int,
    pointer: *mut *mut f64,
    _component_id: c_int,
) -> c_int {
    if let Some(error) = injected_pin_failure() {
        return error;
    }
    unsafe { register(name, direction, pointer, PinKind::Float) }
}

#[no_mangle]
extern "C" fn hal_export_funct(
    name: *const c_char,
    function: Option<unsafe extern "C" fn(*mut c_void, c_long)>,
    argument: *mut c_void,
    uses_fp: c_int,
    reentrant: c_int,
    component_id: c_int,
) -> c_int {
    let name = unsafe { std::ffi::CStr::from_ptr(name) }
        .to_str()
        .expect("function name was UTF-8");
    assert_eq!(name, "dmc2-pendant-control.update");
    assert_eq!(uses_fp, 1);
    assert_eq!(reentrant, 0);
    assert_eq!(component_id, 41);
    HAL_CALLS.lock().expect("HAL call lock").export += 1;
    let result = FAILURE_PLAN
        .lock()
        .expect("failure plan lock")
        .export_result;
    if result != 0 {
        return result;
    }
    let function = function.expect("realtime function was present") as usize;
    *FUNCTION.lock().expect("mock function lock") = Some((function, argument as usize));
    0
}

fn pin<T>(name: &str) -> *mut T {
    let registry = PINS.lock().expect("mock HAL registry lock");
    registry
        .iter()
        .find(|record| record.name == name)
        .map(|record| record.address as *mut T)
        .unwrap_or_else(|| panic!("missing mock HAL pin: {name}"))
}

fn set_bit(name: &str, value: bool) {
    unsafe { ptr::write_volatile(pin::<bool>(name), value) }
}

fn set_s32(name: &str, value: i32) {
    unsafe { ptr::write_volatile(pin::<i32>(name), value) }
}

fn set_u32(name: &str, value: u32) {
    unsafe { ptr::write_volatile(pin::<u32>(name), value) }
}

fn set_float(name: &str, value: f64) {
    unsafe { ptr::write_volatile(pin::<f64>(name), value) }
}

fn get_bit(name: &str) -> bool {
    unsafe { ptr::read_volatile(pin::<bool>(name)) }
}

fn get_s32(name: &str) -> i32 {
    unsafe { ptr::read_volatile(pin::<i32>(name)) }
}

fn get_float(name: &str) -> f64 {
    unsafe { ptr::read_volatile(pin::<f64>(name)) }
}

fn callback() {
    let (function, argument) = FUNCTION
        .lock()
        .expect("mock function lock")
        .expect("realtime function was exported");
    let function: unsafe extern "C" fn(*mut c_void, c_long) = unsafe { mem::transmute(function) };
    unsafe { function(argument as *mut c_void, 1_000_000) };
}

fn publish_pendant(sequence: u32, detent: i32) {
    publish_pendant_state(sequence, detent, 0, 1, true, false, true, false);
}

#[allow(clippy::too_many_arguments)]
fn publish_pendant_state(
    sequence: u32,
    detent: i32,
    axis_code: i32,
    multiplier_code: i32,
    deadman_held: bool,
    estop_pressed: bool,
    selector_valid: bool,
    quadrature_fault: bool,
) {
    let generation = sequence.wrapping_shl(1);
    set_u32("dmc2-pendant-control.snapshot-generation", generation | 1);
    set_bit("dmc2-pendant-control.connected", true);
    set_bit("dmc2-pendant-control.serial-fault", false);
    set_bit("dmc2-pendant-control.quadrature-fault", quadrature_fault);
    set_bit("dmc2-pendant-control.estop-pressed", estop_pressed);
    set_bit("dmc2-pendant-control.deadman-held", deadman_held);
    set_bit("dmc2-pendant-control.selector-valid", selector_valid);
    set_s32("dmc2-pendant-control.axis-code", axis_code);
    set_s32("dmc2-pendant-control.multiplier-code", multiplier_code);
    set_s32("dmc2-pendant-control.latest-detent", detent);
    set_u32("dmc2-pendant-control.quadrature-errors", 0);
    set_u32("dmc2-pendant-control.sequence", sequence);
    set_u32("dmc2-pendant-control.snapshot-generation", generation);
}

fn publish_task(heartbeat: u32) {
    publish_task_state(heartbeat, true, false, true, false, [true; 3]);
}

fn publish_task_state(
    heartbeat: u32,
    machine_on: bool,
    estopped: bool,
    teleop_mode: bool,
    joint_mode: bool,
    homed: [bool; 3],
) {
    let generation = heartbeat.wrapping_shl(1);
    set_u32(
        "dmc2-pendant-control.task-snapshot-generation",
        generation | 1,
    );
    set_bit("dmc2-pendant-control.task-monitor-connected", true);
    set_bit("dmc2-pendant-control.task-monitor-fault", false);
    set_u32("dmc2-pendant-control.task-heartbeat", heartbeat);
    set_bit("dmc2-pendant-control.machine-on", machine_on);
    set_bit("dmc2-pendant-control.estopped", estopped);
    set_bit("dmc2-pendant-control.manual-mode", true);
    set_bit("dmc2-pendant-control.joint-mode", joint_mode);
    set_bit("dmc2-pendant-control.teleop-mode", teleop_mode);
    set_bit("dmc2-pendant-control.interp-idle", true);
    for index in 0..3 {
        set_bit(
            &format!("dmc2-pendant-control.joint-{index}-homed"),
            homed[index],
        );
        set_bit(&format!("dmc2-pendant-control.joint-{index}-homing"), false);
        set_bit(&format!("dmc2-pendant-control.axis-{index}-stopped"), true);
    }
    set_u32("dmc2-pendant-control.task-snapshot-generation", generation);
}

fn reset_mock_hal() {
    rtapi_app_exit();
    unsafe {
        ARENA_OFFSET = 0;
        ptr::write_bytes(ptr::addr_of_mut!(ARENA.0).cast::<u8>(), 0, 131_072);
    }
    PINS.lock().expect("mock registry lock").clear();
    *FUNCTION.lock().expect("mock function lock") = None;
    *FAILURE_PLAN.lock().expect("failure plan lock") = FailurePlan::success();
    *HAL_CALLS.lock().expect("HAL call lock") = HalCalls::new();
    RTAPI_MESSAGES.lock().expect("RTAPI message lock").clear();
}

fn reset_component() {
    reset_mock_hal();
    assert_eq!(rtapi_app_main(), 0);

    set_bit("dmc2-pendant-control.servo-thread-ready", true);
    set_bit("dmc2-pendant-control.ui-ready", true);
    set_bit("dmc2-pendant-control.pendant-mode-enabled", true);
    set_bit("dmc2-pendant-control.mesa-watchdog-has-bit", false);
    set_bit("dmc2-pendant-control.mesa-packet-error-exceeded", false);
}

fn reach_ready(homed: bool) -> (u32, u32) {
    let mut sequence = 1_u32;
    let mut heartbeat = 1_u32;
    let homed_state = [homed; 3];
    for cycle in 0..700 {
        if cycle % 20 == 0 {
            sequence = sequence.wrapping_add(1);
            publish_pendant(sequence, 0);
        }
        heartbeat = heartbeat.wrapping_add(1);
        publish_task_state(heartbeat, true, false, homed, !homed, homed_state);
        set_bit(
            "dmc2-pendant-control.software-watchdog-ok",
            get_bit("dmc2-pendant-control.watchdog-enable"),
        );
        callback();
        if get_bit("dmc2-pendant-control.control-ready") {
            return (sequence, heartbeat);
        }
    }
    panic!("mock realtime component did not reach control-ready");
}

#[allow(clippy::too_many_arguments)]
fn publish_case_cycle(
    sequence: u32,
    heartbeat: u32,
    axis_code: i32,
    multiplier_code: i32,
    detent: i32,
    homed: bool,
    stopped: [bool; 3],
) {
    publish_pendant_state(
        sequence,
        detent,
        axis_code,
        multiplier_code,
        true,
        false,
        true,
        false,
    );
    publish_task_state(heartbeat, true, false, homed, !homed, [homed; 3]);
    for (index, value) in stopped.into_iter().enumerate() {
        set_bit(&format!("dmc2-pendant-control.axis-{index}-stopped"), value);
    }
}

#[test]
fn every_hal_lifecycle_failure_is_returned_and_cleanup_runs_exactly_once() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());

    for (init_result, expected) in [(-19, -19), (0, EINVAL)] {
        reset_mock_hal();
        FAILURE_PLAN.lock().expect("failure plan lock").init_result = init_result;
        assert_eq!(rtapi_app_main(), expected);
        assert_eq!(
            *HAL_CALLS.lock().expect("HAL call lock"),
            HalCalls {
                init: 1,
                ..HalCalls::new()
            }
        );
        rtapi_app_exit();
        assert_eq!(HAL_CALLS.lock().expect("HAL call lock").exit, 0);
    }

    for malloc_failure in 0..2 {
        reset_mock_hal();
        FAILURE_PLAN
            .lock()
            .expect("failure plan lock")
            .malloc_failure = Some(malloc_failure);
        assert_eq!(rtapi_app_main(), ENOMEM);
        let calls = *HAL_CALLS.lock().expect("HAL call lock");
        assert_eq!(calls.init, 1);
        assert_eq!(calls.exit, 1);
        assert_eq!(calls.malloc, malloc_failure + 1);
        assert_eq!(calls.pin, if malloc_failure == 0 { 0 } else { 103 });
        assert_eq!(calls.export, 0);
        assert_eq!(calls.ready, 0);
        rtapi_app_exit();
        assert_eq!(HAL_CALLS.lock().expect("HAL call lock").exit, 1);
    }

    for pin_failure in 0..103 {
        reset_mock_hal();
        let error = -1_000 - pin_failure as c_int;
        FAILURE_PLAN.lock().expect("failure plan lock").pin_failure = Some((pin_failure, error));
        assert_eq!(rtapi_app_main(), error);
        let calls = *HAL_CALLS.lock().expect("HAL call lock");
        assert_eq!(calls.init, 1);
        assert_eq!(calls.exit, 1);
        assert_eq!(calls.malloc, 1);
        assert_eq!(calls.pin, pin_failure + 1);
        assert_eq!(calls.export, 0);
        assert_eq!(calls.ready, 0);
        assert_eq!(PINS.lock().expect("mock registry lock").len(), pin_failure);
        rtapi_app_exit();
        assert_eq!(HAL_CALLS.lock().expect("HAL call lock").exit, 1);
    }

    reset_mock_hal();
    FAILURE_PLAN.lock().expect("failure plan lock").pin_failure = Some((0, 7));
    assert_eq!(rtapi_app_main(), EINVAL);
    assert_eq!(HAL_CALLS.lock().expect("HAL call lock").pin, 1);
    assert_eq!(HAL_CALLS.lock().expect("HAL call lock").exit, 1);

    reset_mock_hal();
    FAILURE_PLAN
        .lock()
        .expect("failure plan lock")
        .export_result = -2_001;
    assert_eq!(rtapi_app_main(), -2_001);
    assert_eq!(
        *HAL_CALLS.lock().expect("HAL call lock"),
        HalCalls {
            init: 1,
            exit: 1,
            ready: 0,
            malloc: 2,
            pin: 103,
            export: 1,
        }
    );
    rtapi_app_exit();
    assert_eq!(HAL_CALLS.lock().expect("HAL call lock").exit, 1);

    reset_mock_hal();
    FAILURE_PLAN
        .lock()
        .expect("failure plan lock")
        .export_result = 7;
    assert_eq!(rtapi_app_main(), EINVAL);
    assert_eq!(HAL_CALLS.lock().expect("HAL call lock").export, 1);
    assert_eq!(HAL_CALLS.lock().expect("HAL call lock").exit, 1);

    reset_mock_hal();
    FAILURE_PLAN.lock().expect("failure plan lock").ready_result = -2_002;
    assert_eq!(rtapi_app_main(), -2_002);
    assert_eq!(
        *HAL_CALLS.lock().expect("HAL call lock"),
        HalCalls {
            init: 1,
            exit: 1,
            ready: 1,
            malloc: 2,
            pin: 103,
            export: 1,
        }
    );
    rtapi_app_exit();
    assert_eq!(HAL_CALLS.lock().expect("HAL call lock").exit, 1);

    reset_mock_hal();
    FAILURE_PLAN.lock().expect("failure plan lock").ready_result = 7;
    assert_eq!(rtapi_app_main(), EINVAL);
    assert_eq!(HAL_CALLS.lock().expect("HAL call lock").ready, 1);
    assert_eq!(HAL_CALLS.lock().expect("HAL call lock").exit, 1);

    reset_mock_hal();
    {
        let mut plan = FAILURE_PLAN.lock().expect("failure plan lock");
        plan.malloc_failure = Some(0);
        plan.exit_result = -2_003;
    }
    assert_eq!(rtapi_app_main(), ENOMEM);
    assert_eq!(HAL_CALLS.lock().expect("HAL call lock").exit, 1);
    assert_eq!(
        *RTAPI_MESSAGES.lock().expect("RTAPI message lock"),
        vec![(
            hal::msg_level_t_RTAPI_MSG_ERR,
            "dmc2_rt: ERROR: hal_exit() failed\n".to_string(),
        )]
    );
    rtapi_app_exit();
    assert_eq!(HAL_CALLS.lock().expect("HAL call lock").exit, 1);

    reset_mock_hal();
    FAILURE_PLAN.lock().expect("failure plan lock").exit_result = -2_004;
    assert_eq!(rtapi_app_main(), 0);
    rtapi_app_exit();
    assert_eq!(HAL_CALLS.lock().expect("HAL call lock").exit, 1);
    assert_eq!(RTAPI_MESSAGES.lock().expect("RTAPI message lock").len(), 1);
    rtapi_app_exit();
    assert_eq!(HAL_CALLS.lock().expect("HAL call lock").exit, 1);
    assert_eq!(RTAPI_MESSAGES.lock().expect("RTAPI message lock").len(), 1);
}

#[test]
fn every_axis_scale_direction_and_linuxcnc_jog_mode_reaches_the_exact_hal_edge() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let axes = [(0, 0, 1, -1), (1, 1, 0, 1), (2, 2, 2, 1)];
    let multipliers = [(1, 10, 300.0), (10, 100, 4_500.0), (100, 1_000, 18_000.0)];

    for homed in [false, true] {
        for (axis_code, axis_index, motor, clockwise_sign) in axes {
            for (multiplier_code, pulses, speed) in multipliers {
                for detent in [-1, 1] {
                    reset_component();
                    let (mut sequence, mut heartbeat) = reach_ready(homed);

                    for _ in 0..2 {
                        sequence = sequence.wrapping_add(1);
                        heartbeat = heartbeat.wrapping_add(1);
                        publish_case_cycle(
                            sequence,
                            heartbeat,
                            axis_code,
                            multiplier_code,
                            0,
                            homed,
                            [true; 3],
                        );
                        callback();
                    }

                    sequence = sequence.wrapping_add(1);
                    heartbeat = heartbeat.wrapping_add(1);
                    publish_case_cycle(
                        sequence,
                        heartbeat,
                        axis_code,
                        multiplier_code,
                        detent,
                        homed,
                        [true; 3],
                    );
                    callback();

                    let delta = detent * clockwise_sign * pulses;
                    let path = if homed { "axis" } else { "joint" };
                    let opposite_path = if homed { "joint" } else { "axis" };
                    let asserted_suffix = if delta > 0 {
                        "increment-plus"
                    } else {
                        "increment-minus"
                    };
                    assert!(get_bit(&format!(
                        "dmc2-pendant-control.{path}-{axis_index}-{asserted_suffix}"
                    )));
                    assert!(!get_bit(&format!(
                        "dmc2-pendant-control.{opposite_path}-{axis_index}-{asserted_suffix}"
                    )));
                    let increment = get_float(&format!(
                        "dmc2-pendant-control.{path}-{axis_index}-increment"
                    ));
                    assert!((increment - f64::from(pulses) / 1_000.0).abs() < 1e-12);
                    assert_eq!(
                        get_float(&format!("dmc2-pendant-control.{path}-jog-speed")),
                        speed
                    );
                    assert!(get_bit(&format!(
                        "dmc2-pendant-control.motor-{motor}-command-enable"
                    )));

                    for cycle in 0..85 {
                        if cycle % 20 == 0 {
                            sequence = sequence.wrapping_add(1);
                        }
                        heartbeat = heartbeat.wrapping_add(1);
                        let mut stopped = [true; 3];
                        stopped[axis_index] = cycle >= 50;
                        publish_case_cycle(
                            sequence,
                            heartbeat,
                            axis_code,
                            multiplier_code,
                            0,
                            homed,
                            stopped,
                        );
                        if cycle == 50 {
                            set_s32(&format!("dmc2-pendant-control.motor-{motor}-count"), delta);
                            set_float(
                                &format!("dmc2-pendant-control.motor-{motor}-position-feedback"),
                                f64::from(delta) / 1_000.0,
                            );
                        }
                        callback();
                    }
                    assert!(!get_bit("dmc2-pendant-control.fault"));
                    assert!(!get_bit("dmc2-pendant-control.jog-active"));
                    assert_eq!(get_s32("dmc2-pendant-control.fault-code"), 0);
                }
            }
        }
    }
    rtapi_app_exit();
}

#[test]
fn exported_component_reaches_ready_and_emits_one_exact_x1_linuxcnc_edge() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    reset_component();
    let (mut sequence, mut heartbeat) = reach_ready(true);
    assert!(get_bit("dmc2-pendant-control.control-ready"));
    assert!(get_bit("dmc2-pendant-control.external-enable"));
    assert!(!get_bit("dmc2-pendant-control.fault"));

    sequence = sequence.wrapping_add(1);
    publish_pendant(sequence, 0);
    heartbeat = heartbeat.wrapping_add(1);
    publish_task(heartbeat);
    callback();
    sequence = sequence.wrapping_add(1);
    publish_pendant(sequence, 0);
    heartbeat = heartbeat.wrapping_add(1);
    publish_task(heartbeat);
    callback();
    sequence = sequence.wrapping_add(1);
    publish_pendant(sequence, 1);
    heartbeat = heartbeat.wrapping_add(1);
    publish_task(heartbeat);
    callback();

    assert!(get_bit("dmc2-pendant-control.axis-0-increment-minus"));
    assert!(!get_bit("dmc2-pendant-control.axis-0-increment-plus"));
    assert_eq!(get_float("dmc2-pendant-control.axis-0-increment"), 0.01);
    assert_eq!(get_float("dmc2-pendant-control.axis-jog-speed"), 300.0);

    for cycle in 0..80 {
        if cycle % 20 == 0 {
            sequence = sequence.wrapping_add(1);
            publish_pendant(sequence, 0);
        }
        heartbeat = heartbeat.wrapping_add(1);
        publish_task(heartbeat);
        set_bit("dmc2-pendant-control.axis-0-stopped", cycle >= 50);
        if cycle == 50 {
            set_s32("dmc2-pendant-control.motor-1-count", -11);
            set_float(
                "dmc2-pendant-control.motor-1-position-feedback",
                -0.010_003_16,
            );
        }
        callback();
    }
    assert!(!get_bit("dmc2-pendant-control.axis-0-increment-minus"));
    assert!(!get_bit("dmc2-pendant-control.jog-active"));
    assert!(!get_bit("dmc2-pendant-control.fault"));
    assert_eq!(get_s32("dmc2-pendant-control.fault-code"), 0);
    rtapi_app_exit();
}
