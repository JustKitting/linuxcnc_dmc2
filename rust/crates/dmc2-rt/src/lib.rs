// Release builds are the installable realtime module and are deliberately
// no_std. Debug/test builds use std only so Cargo's test harness can link.
#![cfg_attr(not(debug_assertions), no_std)]

use core::ffi::{c_char, c_int, c_long, c_void};
use core::mem;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

use dmc2_core::halui::HaluiCommandSequencer;
use dmc2_core::pendant::{AxisSelector, MultiplierSelector, PendantSample};
use dmc2_core::runtime::{RuntimeController, RuntimeInputs, RuntimeOutputs};
use dmc2_core::supervisor::{FaultCode, MachineSnapshot};
use dmc2_core::PULSES_PER_MM;
use dmc2_hal_sys as hal;

const COMPONENT_NAME: &[u8] = b"dmc2-pendant-control\0";
const FUNCTION_NAME: &[u8] = b"dmc2-pendant-control.update\0";
const ENOMEM: c_int = -12;
const EINVAL: c_int = -22;

static mut COMPONENT_ID: c_int = -1;

struct Pins {
    pendant_snapshot_generation: *mut hal::hal_u32_t,
    connected: *mut hal::hal_bit_t,
    serial_fault: *mut hal::hal_bit_t,
    quadrature_fault: *mut hal::hal_bit_t,
    estop_pressed: *mut hal::hal_bit_t,
    deadman_held: *mut hal::hal_bit_t,
    selector_valid: *mut hal::hal_bit_t,
    axis_code: *mut hal::hal_s32_t,
    multiplier_code: *mut hal::hal_s32_t,
    latest_detent: *mut hal::hal_s32_t,
    detent_count: *mut hal::hal_s32_t,
    transition_count: *mut hal::hal_s32_t,
    quadrature_errors: *mut hal::hal_u32_t,
    sequence: *mut hal::hal_u32_t,
    milliseconds: *mut hal::hal_u32_t,

    task_snapshot_generation: *mut hal::hal_u32_t,
    task_monitor_connected: *mut hal::hal_bit_t,
    task_monitor_fault: *mut hal::hal_bit_t,
    task_heartbeat: *mut hal::hal_u32_t,
    machine_on: *mut hal::hal_bit_t,
    estopped: *mut hal::hal_bit_t,
    manual_mode: *mut hal::hal_bit_t,
    joint_mode: *mut hal::hal_bit_t,
    teleop_mode: *mut hal::hal_bit_t,
    interp_idle: *mut hal::hal_bit_t,
    joint_homed: [*mut hal::hal_bit_t; 3],
    joint_homing: [*mut hal::hal_bit_t; 3],
    axis_stopped: [*mut hal::hal_bit_t; 3],

    motor_count: [*mut hal::hal_s32_t; 3],
    motor_position_feedback: [*mut hal::real_t; 3],
    motor_limit_raw: [*mut hal::hal_bit_t; 3],
    motor_limit_latched: [*mut hal::hal_bit_t; 3],
    pendant_mode_enabled: *mut hal::hal_bit_t,
    servo_thread_ready: *mut hal::hal_bit_t,
    mesa_watchdog_has_bit: *mut hal::hal_bit_t,
    mesa_packet_error: *mut hal::hal_bit_t,
    mesa_packet_error_total: *mut hal::hal_u32_t,
    mesa_packet_error_exceeded: *mut hal::hal_bit_t,
    software_watchdog_ok: *mut hal::hal_bit_t,
    ui_ready: *mut hal::hal_bit_t,

    motor_limit_reset: [*mut hal::hal_bit_t; 3],
    motor_command_enable: [*mut hal::hal_bit_t; 3],
    motor_toward_limit: [*mut hal::hal_bit_t; 3],
    external_enable: *mut hal::hal_bit_t,
    watchdog_enable: *mut hal::hal_bit_t,
    heartbeat: *mut hal::hal_bit_t,
    position_known: *mut hal::hal_bit_t,
    position_unknown: *mut hal::hal_bit_t,
    control_available: *mut hal::hal_bit_t,
    control_ready: *mut hal::hal_bit_t,
    fault: *mut hal::hal_bit_t,
    fault_code: *mut hal::hal_s32_t,
    recovery_active: *mut hal::hal_bit_t,
    jog_active: *mut hal::hal_bit_t,
    bounce_active: *mut hal::hal_bit_t,
    supervisor_phase: *mut hal::hal_s32_t,
    mesa_phase: *mut hal::hal_s32_t,
    controller_watchdog_phase: *mut hal::hal_s32_t,
    estop_reset_request: *mut hal::hal_bit_t,
    machine_on_request: *mut hal::hal_bit_t,

    axis_increment_plus: [*mut hal::hal_bit_t; 3],
    axis_increment_minus: [*mut hal::hal_bit_t; 3],
    joint_increment_plus: [*mut hal::hal_bit_t; 3],
    joint_increment_minus: [*mut hal::hal_bit_t; 3],
    axis_increment: [*mut hal::real_t; 3],
    joint_increment: [*mut hal::real_t; 3],
    axis_jog_speed: *mut hal::real_t,
    joint_jog_speed: *mut hal::real_t,
    jog_stop: *mut hal::hal_bit_t,
    jog_stop_immediate: *mut hal::hal_bit_t,
    command_phase: *mut hal::hal_s32_t,
}

#[derive(Clone, Copy)]
struct CachedTaskSnapshot {
    connected: bool,
    fault: bool,
    heartbeat: u32,
    machine: MachineSnapshot,
}

impl CachedTaskSnapshot {
    const fn safe() -> Self {
        Self {
            connected: false,
            fault: true,
            heartbeat: 0,
            machine: MachineSnapshot {
                machine_on: false,
                estopped: true,
                manual_mode: false,
                joint_mode: false,
                teleop_mode: false,
                interp_idle: false,
                homed: [false; 3],
                homing: [false; 3],
                axis_stopped: [true; 3],
            },
        }
    }
}

struct ComponentState {
    pins: *mut Pins,
    runtime: RuntimeController,
    sequencer: HaluiCommandSequencer,
    task: CachedTaskSnapshot,
}

impl ComponentState {
    const fn new(pins: *mut Pins) -> Self {
        Self {
            pins,
            runtime: RuntimeController::new(),
            sequencer: HaluiCommandSequencer::new(),
            task: CachedTaskSnapshot::safe(),
        }
    }
}

#[cfg(not(debug_assertions))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    unsafe extern "C" {
        fn abort() -> !;
    }
    unsafe { abort() }
}

unsafe fn bit_pin(
    pins: *mut *mut hal::hal_bit_t,
    name: &'static [u8],
    direction: hal::hal_pin_dir_t,
    component_id: c_int,
) -> Result<(), c_int> {
    let result = unsafe {
        hal::hal_pin_bit_new(
            name.as_ptr().cast::<c_char>(),
            direction,
            pins,
            component_id,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(result)
    }
}

unsafe fn s32_pin(
    pins: *mut *mut hal::hal_s32_t,
    name: &'static [u8],
    direction: hal::hal_pin_dir_t,
    component_id: c_int,
) -> Result<(), c_int> {
    let result = unsafe {
        hal::hal_pin_s32_new(
            name.as_ptr().cast::<c_char>(),
            direction,
            pins,
            component_id,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(result)
    }
}

unsafe fn u32_pin(
    pins: *mut *mut hal::hal_u32_t,
    name: &'static [u8],
    direction: hal::hal_pin_dir_t,
    component_id: c_int,
) -> Result<(), c_int> {
    let result = unsafe {
        hal::hal_pin_u32_new(
            name.as_ptr().cast::<c_char>(),
            direction,
            pins,
            component_id,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(result)
    }
}

unsafe fn float_pin(
    pins: *mut *mut hal::real_t,
    name: &'static [u8],
    direction: hal::hal_pin_dir_t,
    component_id: c_int,
) -> Result<(), c_int> {
    let result = unsafe {
        hal::hal_pin_float_new(
            name.as_ptr().cast::<c_char>(),
            direction,
            pins,
            component_id,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(result)
    }
}

macro_rules! bit {
    ($pins:expr, $field:ident, $name:literal, $direction:expr, $id:expr) => {
        unsafe {
            bit_pin(
                &mut (*$pins).$field,
                concat!($name, "\0").as_bytes(),
                $direction,
                $id,
            )?
        }
    };
}

macro_rules! s32 {
    ($pins:expr, $field:ident, $name:literal, $direction:expr, $id:expr) => {
        unsafe {
            s32_pin(
                &mut (*$pins).$field,
                concat!($name, "\0").as_bytes(),
                $direction,
                $id,
            )?
        }
    };
}

macro_rules! u32_pin_field {
    ($pins:expr, $field:ident, $name:literal, $direction:expr, $id:expr) => {
        unsafe {
            u32_pin(
                &mut (*$pins).$field,
                concat!($name, "\0").as_bytes(),
                $direction,
                $id,
            )?
        }
    };
}

macro_rules! float {
    ($pins:expr, $field:ident, $name:literal, $direction:expr, $id:expr) => {
        unsafe {
            float_pin(
                &mut (*$pins).$field,
                concat!($name, "\0").as_bytes(),
                $direction,
                $id,
            )?
        }
    };
}

macro_rules! bit_index {
    ($pins:expr, $field:ident, $index:expr, $name:literal, $direction:expr, $id:expr) => {
        unsafe {
            bit_pin(
                &mut (*$pins).$field[$index],
                concat!($name, "\0").as_bytes(),
                $direction,
                $id,
            )?
        }
    };
}

macro_rules! s32_index {
    ($pins:expr, $field:ident, $index:expr, $name:literal, $direction:expr, $id:expr) => {
        unsafe {
            s32_pin(
                &mut (*$pins).$field[$index],
                concat!($name, "\0").as_bytes(),
                $direction,
                $id,
            )?
        }
    };
}

macro_rules! float_index {
    ($pins:expr, $field:ident, $index:expr, $name:literal, $direction:expr, $id:expr) => {
        unsafe {
            float_pin(
                &mut (*$pins).$field[$index],
                concat!($name, "\0").as_bytes(),
                $direction,
                $id,
            )?
        }
    };
}

unsafe fn register_pins(pins: *mut Pins, component_id: c_int) -> Result<(), c_int> {
    let input = hal::hal_pin_dir_t_HAL_IN;
    let output = hal::hal_pin_dir_t_HAL_OUT;
    let io = hal::hal_pin_dir_t_HAL_IO;

    u32_pin_field!(
        pins,
        pendant_snapshot_generation,
        "dmc2-pendant-control.snapshot-generation",
        input,
        component_id
    );
    bit!(
        pins,
        connected,
        "dmc2-pendant-control.connected",
        input,
        component_id
    );
    bit!(
        pins,
        serial_fault,
        "dmc2-pendant-control.serial-fault",
        input,
        component_id
    );
    bit!(
        pins,
        quadrature_fault,
        "dmc2-pendant-control.quadrature-fault",
        input,
        component_id
    );
    bit!(
        pins,
        estop_pressed,
        "dmc2-pendant-control.estop-pressed",
        input,
        component_id
    );
    bit!(
        pins,
        deadman_held,
        "dmc2-pendant-control.deadman-held",
        input,
        component_id
    );
    bit!(
        pins,
        selector_valid,
        "dmc2-pendant-control.selector-valid",
        input,
        component_id
    );
    s32!(
        pins,
        axis_code,
        "dmc2-pendant-control.axis-code",
        input,
        component_id
    );
    s32!(
        pins,
        multiplier_code,
        "dmc2-pendant-control.multiplier-code",
        input,
        component_id
    );
    s32!(
        pins,
        latest_detent,
        "dmc2-pendant-control.latest-detent",
        input,
        component_id
    );
    s32!(
        pins,
        detent_count,
        "dmc2-pendant-control.detent-count",
        input,
        component_id
    );
    s32!(
        pins,
        transition_count,
        "dmc2-pendant-control.transition-count",
        input,
        component_id
    );
    u32_pin_field!(
        pins,
        quadrature_errors,
        "dmc2-pendant-control.quadrature-errors",
        input,
        component_id
    );
    u32_pin_field!(
        pins,
        sequence,
        "dmc2-pendant-control.sequence",
        input,
        component_id
    );
    u32_pin_field!(
        pins,
        milliseconds,
        "dmc2-pendant-control.milliseconds",
        input,
        component_id
    );

    u32_pin_field!(
        pins,
        task_snapshot_generation,
        "dmc2-pendant-control.task-snapshot-generation",
        input,
        component_id
    );
    bit!(
        pins,
        task_monitor_connected,
        "dmc2-pendant-control.task-monitor-connected",
        input,
        component_id
    );
    bit!(
        pins,
        task_monitor_fault,
        "dmc2-pendant-control.task-monitor-fault",
        input,
        component_id
    );
    u32_pin_field!(
        pins,
        task_heartbeat,
        "dmc2-pendant-control.task-heartbeat",
        input,
        component_id
    );
    bit!(
        pins,
        machine_on,
        "dmc2-pendant-control.machine-on",
        input,
        component_id
    );
    bit!(
        pins,
        estopped,
        "dmc2-pendant-control.estopped",
        input,
        component_id
    );
    bit!(
        pins,
        manual_mode,
        "dmc2-pendant-control.manual-mode",
        input,
        component_id
    );
    bit!(
        pins,
        joint_mode,
        "dmc2-pendant-control.joint-mode",
        input,
        component_id
    );
    bit!(
        pins,
        teleop_mode,
        "dmc2-pendant-control.teleop-mode",
        input,
        component_id
    );
    bit!(
        pins,
        interp_idle,
        "dmc2-pendant-control.interp-idle",
        input,
        component_id
    );
    bit_index!(
        pins,
        joint_homed,
        0,
        "dmc2-pendant-control.joint-0-homed",
        input,
        component_id
    );
    bit_index!(
        pins,
        joint_homed,
        1,
        "dmc2-pendant-control.joint-1-homed",
        input,
        component_id
    );
    bit_index!(
        pins,
        joint_homed,
        2,
        "dmc2-pendant-control.joint-2-homed",
        input,
        component_id
    );
    bit_index!(
        pins,
        joint_homing,
        0,
        "dmc2-pendant-control.joint-0-homing",
        input,
        component_id
    );
    bit_index!(
        pins,
        joint_homing,
        1,
        "dmc2-pendant-control.joint-1-homing",
        input,
        component_id
    );
    bit_index!(
        pins,
        joint_homing,
        2,
        "dmc2-pendant-control.joint-2-homing",
        input,
        component_id
    );
    bit_index!(
        pins,
        axis_stopped,
        0,
        "dmc2-pendant-control.axis-0-stopped",
        input,
        component_id
    );
    bit_index!(
        pins,
        axis_stopped,
        1,
        "dmc2-pendant-control.axis-1-stopped",
        input,
        component_id
    );
    bit_index!(
        pins,
        axis_stopped,
        2,
        "dmc2-pendant-control.axis-2-stopped",
        input,
        component_id
    );

    s32_index!(
        pins,
        motor_count,
        0,
        "dmc2-pendant-control.motor-0-count",
        input,
        component_id
    );
    s32_index!(
        pins,
        motor_count,
        1,
        "dmc2-pendant-control.motor-1-count",
        input,
        component_id
    );
    s32_index!(
        pins,
        motor_count,
        2,
        "dmc2-pendant-control.motor-2-count",
        input,
        component_id
    );
    float_index!(
        pins,
        motor_position_feedback,
        0,
        "dmc2-pendant-control.motor-0-position-feedback",
        input,
        component_id
    );
    float_index!(
        pins,
        motor_position_feedback,
        1,
        "dmc2-pendant-control.motor-1-position-feedback",
        input,
        component_id
    );
    float_index!(
        pins,
        motor_position_feedback,
        2,
        "dmc2-pendant-control.motor-2-position-feedback",
        input,
        component_id
    );
    bit_index!(
        pins,
        motor_limit_raw,
        0,
        "dmc2-pendant-control.motor-0-limit-raw",
        input,
        component_id
    );
    bit_index!(
        pins,
        motor_limit_raw,
        1,
        "dmc2-pendant-control.motor-1-limit-raw",
        input,
        component_id
    );
    bit_index!(
        pins,
        motor_limit_raw,
        2,
        "dmc2-pendant-control.motor-2-limit-raw",
        input,
        component_id
    );
    bit_index!(
        pins,
        motor_limit_latched,
        0,
        "dmc2-pendant-control.motor-0-limit-latched",
        input,
        component_id
    );
    bit_index!(
        pins,
        motor_limit_latched,
        1,
        "dmc2-pendant-control.motor-1-limit-latched",
        input,
        component_id
    );
    bit_index!(
        pins,
        motor_limit_latched,
        2,
        "dmc2-pendant-control.motor-2-limit-latched",
        input,
        component_id
    );
    bit!(
        pins,
        pendant_mode_enabled,
        "dmc2-pendant-control.pendant-mode-enabled",
        input,
        component_id
    );
    bit!(
        pins,
        servo_thread_ready,
        "dmc2-pendant-control.servo-thread-ready",
        input,
        component_id
    );
    bit!(
        pins,
        mesa_watchdog_has_bit,
        "dmc2-pendant-control.mesa-watchdog-has-bit",
        io,
        component_id
    );
    bit!(
        pins,
        mesa_packet_error,
        "dmc2-pendant-control.mesa-packet-error",
        input,
        component_id
    );
    u32_pin_field!(
        pins,
        mesa_packet_error_total,
        "dmc2-pendant-control.mesa-packet-error-total",
        input,
        component_id
    );
    bit!(
        pins,
        mesa_packet_error_exceeded,
        "dmc2-pendant-control.mesa-packet-error-exceeded",
        input,
        component_id
    );
    bit!(
        pins,
        software_watchdog_ok,
        "dmc2-pendant-control.software-watchdog-ok",
        input,
        component_id
    );
    bit!(
        pins,
        ui_ready,
        "dmc2-pendant-control.ui-ready",
        input,
        component_id
    );

    bit_index!(
        pins,
        motor_limit_reset,
        0,
        "dmc2-pendant-control.motor-0-limit-reset",
        output,
        component_id
    );
    bit_index!(
        pins,
        motor_limit_reset,
        1,
        "dmc2-pendant-control.motor-1-limit-reset",
        output,
        component_id
    );
    bit_index!(
        pins,
        motor_limit_reset,
        2,
        "dmc2-pendant-control.motor-2-limit-reset",
        output,
        component_id
    );
    bit_index!(
        pins,
        motor_command_enable,
        0,
        "dmc2-pendant-control.motor-0-command-enable",
        output,
        component_id
    );
    bit_index!(
        pins,
        motor_command_enable,
        1,
        "dmc2-pendant-control.motor-1-command-enable",
        output,
        component_id
    );
    bit_index!(
        pins,
        motor_command_enable,
        2,
        "dmc2-pendant-control.motor-2-command-enable",
        output,
        component_id
    );
    bit_index!(
        pins,
        motor_toward_limit,
        0,
        "dmc2-pendant-control.motor-0-toward-limit",
        output,
        component_id
    );
    bit_index!(
        pins,
        motor_toward_limit,
        1,
        "dmc2-pendant-control.motor-1-toward-limit",
        output,
        component_id
    );
    bit_index!(
        pins,
        motor_toward_limit,
        2,
        "dmc2-pendant-control.motor-2-toward-limit",
        output,
        component_id
    );
    bit!(
        pins,
        external_enable,
        "dmc2-pendant-control.external-enable",
        output,
        component_id
    );
    bit!(
        pins,
        watchdog_enable,
        "dmc2-pendant-control.watchdog-enable",
        output,
        component_id
    );
    bit!(
        pins,
        heartbeat,
        "dmc2-pendant-control.heartbeat",
        output,
        component_id
    );
    bit!(
        pins,
        position_known,
        "dmc2-pendant-control.position-known",
        output,
        component_id
    );
    bit!(
        pins,
        position_unknown,
        "dmc2-pendant-control.position-unknown",
        output,
        component_id
    );
    bit!(
        pins,
        control_available,
        "dmc2-pendant-control.control-available",
        output,
        component_id
    );
    bit!(
        pins,
        control_ready,
        "dmc2-pendant-control.control-ready",
        output,
        component_id
    );
    bit!(
        pins,
        fault,
        "dmc2-pendant-control.fault",
        output,
        component_id
    );
    s32!(
        pins,
        fault_code,
        "dmc2-pendant-control.fault-code",
        output,
        component_id
    );
    bit!(
        pins,
        recovery_active,
        "dmc2-pendant-control.recovery-active",
        output,
        component_id
    );
    bit!(
        pins,
        jog_active,
        "dmc2-pendant-control.jog-active",
        output,
        component_id
    );
    bit!(
        pins,
        bounce_active,
        "dmc2-pendant-control.bounce-active",
        output,
        component_id
    );
    s32!(
        pins,
        supervisor_phase,
        "dmc2-pendant-control.supervisor-phase",
        output,
        component_id
    );
    s32!(
        pins,
        mesa_phase,
        "dmc2-pendant-control.mesa-phase",
        output,
        component_id
    );
    s32!(
        pins,
        controller_watchdog_phase,
        "dmc2-pendant-control.controller-watchdog-phase",
        output,
        component_id
    );
    bit!(
        pins,
        estop_reset_request,
        "dmc2-pendant-control.estop-reset-request",
        output,
        component_id
    );
    bit!(
        pins,
        machine_on_request,
        "dmc2-pendant-control.machine-on-request",
        output,
        component_id
    );

    bit_index!(
        pins,
        axis_increment_plus,
        0,
        "dmc2-pendant-control.axis-0-increment-plus",
        output,
        component_id
    );
    bit_index!(
        pins,
        axis_increment_plus,
        1,
        "dmc2-pendant-control.axis-1-increment-plus",
        output,
        component_id
    );
    bit_index!(
        pins,
        axis_increment_plus,
        2,
        "dmc2-pendant-control.axis-2-increment-plus",
        output,
        component_id
    );
    bit_index!(
        pins,
        axis_increment_minus,
        0,
        "dmc2-pendant-control.axis-0-increment-minus",
        output,
        component_id
    );
    bit_index!(
        pins,
        axis_increment_minus,
        1,
        "dmc2-pendant-control.axis-1-increment-minus",
        output,
        component_id
    );
    bit_index!(
        pins,
        axis_increment_minus,
        2,
        "dmc2-pendant-control.axis-2-increment-minus",
        output,
        component_id
    );
    bit_index!(
        pins,
        joint_increment_plus,
        0,
        "dmc2-pendant-control.joint-0-increment-plus",
        output,
        component_id
    );
    bit_index!(
        pins,
        joint_increment_plus,
        1,
        "dmc2-pendant-control.joint-1-increment-plus",
        output,
        component_id
    );
    bit_index!(
        pins,
        joint_increment_plus,
        2,
        "dmc2-pendant-control.joint-2-increment-plus",
        output,
        component_id
    );
    bit_index!(
        pins,
        joint_increment_minus,
        0,
        "dmc2-pendant-control.joint-0-increment-minus",
        output,
        component_id
    );
    bit_index!(
        pins,
        joint_increment_minus,
        1,
        "dmc2-pendant-control.joint-1-increment-minus",
        output,
        component_id
    );
    bit_index!(
        pins,
        joint_increment_minus,
        2,
        "dmc2-pendant-control.joint-2-increment-minus",
        output,
        component_id
    );
    float_index!(
        pins,
        axis_increment,
        0,
        "dmc2-pendant-control.axis-0-increment",
        output,
        component_id
    );
    float_index!(
        pins,
        axis_increment,
        1,
        "dmc2-pendant-control.axis-1-increment",
        output,
        component_id
    );
    float_index!(
        pins,
        axis_increment,
        2,
        "dmc2-pendant-control.axis-2-increment",
        output,
        component_id
    );
    float_index!(
        pins,
        joint_increment,
        0,
        "dmc2-pendant-control.joint-0-increment",
        output,
        component_id
    );
    float_index!(
        pins,
        joint_increment,
        1,
        "dmc2-pendant-control.joint-1-increment",
        output,
        component_id
    );
    float_index!(
        pins,
        joint_increment,
        2,
        "dmc2-pendant-control.joint-2-increment",
        output,
        component_id
    );
    float!(
        pins,
        axis_jog_speed,
        "dmc2-pendant-control.axis-jog-speed",
        output,
        component_id
    );
    float!(
        pins,
        joint_jog_speed,
        "dmc2-pendant-control.joint-jog-speed",
        output,
        component_id
    );
    bit!(
        pins,
        jog_stop,
        "dmc2-pendant-control.jog-stop",
        output,
        component_id
    );
    bit!(
        pins,
        jog_stop_immediate,
        "dmc2-pendant-control.jog-stop-immediate",
        output,
        component_id
    );
    s32!(
        pins,
        command_phase,
        "dmc2-pendant-control.command-phase",
        output,
        component_id
    );
    Ok(())
}

unsafe fn read<T: Copy>(pointer: *mut T) -> T {
    unsafe { ptr::read_volatile(pointer) }
}

unsafe fn write<T: Copy>(pointer: *mut T, value: T) {
    unsafe { ptr::write_volatile(pointer, value) }
}

unsafe fn generation(pointer: *mut hal::hal_u32_t) -> u32 {
    unsafe { (&*pointer.cast::<AtomicU32>()).load(Ordering::SeqCst) }
}

unsafe fn read_pendant(pins: &Pins) -> (bool, PendantSample, bool, bool, bool) {
    let first = unsafe { generation(pins.pendant_snapshot_generation) };
    if first & 1 != 0 {
        return (false, safe_pendant(), false, true, false);
    }
    let connected = unsafe { read(pins.connected) };
    let serial_fault = unsafe { read(pins.serial_fault) };
    let quadrature_fault = unsafe { read(pins.quadrature_fault) };
    let axis_code = unsafe { read(pins.axis_code) };
    let multiplier_code = unsafe { read(pins.multiplier_code) };
    let sample = PendantSample {
        sequence: unsafe { read(pins.sequence) },
        quadrature_errors: unsafe { read(pins.quadrature_errors) },
        latest_detent: unsafe { read(pins.latest_detent) },
        axis: AxisSelector::from_wire_code(axis_code).unwrap_or(AxisSelector::Invalid),
        multiplier: MultiplierSelector::from_wire_code(multiplier_code)
            .unwrap_or(MultiplierSelector::Invalid),
        deadman_held: unsafe { read(pins.deadman_held) },
        estop_pressed: unsafe { read(pins.estop_pressed) },
        selector_valid: unsafe { read(pins.selector_valid) },
    };
    let second = unsafe { generation(pins.pendant_snapshot_generation) };
    (
        first == second && second & 1 == 0,
        sample,
        connected,
        serial_fault,
        quadrature_fault,
    )
}

const fn safe_pendant() -> PendantSample {
    PendantSample {
        sequence: 0,
        quadrature_errors: 0,
        latest_detent: 0,
        axis: AxisSelector::Invalid,
        multiplier: MultiplierSelector::Invalid,
        deadman_held: false,
        estop_pressed: true,
        selector_valid: false,
    }
}

unsafe fn refresh_task_snapshot(pins: &Pins, cached: &mut CachedTaskSnapshot) -> bool {
    let first = unsafe { generation(pins.task_snapshot_generation) };
    if first & 1 != 0 {
        return false;
    }
    let value = CachedTaskSnapshot {
        connected: unsafe { read(pins.task_monitor_connected) },
        fault: unsafe { read(pins.task_monitor_fault) },
        heartbeat: unsafe { read(pins.task_heartbeat) },
        machine: MachineSnapshot {
            machine_on: unsafe { read(pins.machine_on) },
            estopped: unsafe { read(pins.estopped) },
            manual_mode: unsafe { read(pins.manual_mode) },
            joint_mode: unsafe { read(pins.joint_mode) },
            teleop_mode: unsafe { read(pins.teleop_mode) },
            interp_idle: unsafe { read(pins.interp_idle) },
            homed: [
                unsafe { read(pins.joint_homed[0]) },
                unsafe { read(pins.joint_homed[1]) },
                unsafe { read(pins.joint_homed[2]) },
            ],
            homing: [
                unsafe { read(pins.joint_homing[0]) },
                unsafe { read(pins.joint_homing[1]) },
                unsafe { read(pins.joint_homing[2]) },
            ],
            axis_stopped: [
                unsafe { read(pins.axis_stopped[0]) },
                unsafe { read(pins.axis_stopped[1]) },
                unsafe { read(pins.axis_stopped[2]) },
            ],
        },
    };
    let second = unsafe { generation(pins.task_snapshot_generation) };
    if first == second && second & 1 == 0 {
        *cached = value;
        true
    } else {
        false
    }
}

unsafe fn runtime_inputs(
    state: &mut ComponentState,
    pins: &Pins,
    command_channel_ready: bool,
) -> RuntimeInputs {
    let (pendant_coherent, pendant_sample, connected, serial_fault, quadrature_fault) =
        unsafe { read_pendant(pins) };
    let _task_coherent = unsafe { refresh_task_snapshot(pins, &mut state.task) };
    RuntimeInputs {
        servo_thread_ready: unsafe { read(pins.servo_thread_ready) },
        mesa_watchdog_has_bit: unsafe { read(pins.mesa_watchdog_has_bit) },
        mesa_io_error: unsafe { read(pins.mesa_packet_error_exceeded) },
        software_watchdog_ok: unsafe { read(pins.software_watchdog_ok) },
        ui_ready: unsafe { read(pins.ui_ready) },
        task_monitor_connected: state.task.connected,
        task_monitor_fault: state.task.fault,
        task_heartbeat: state.task.heartbeat,
        pendant_coherent,
        pendant_connected: connected,
        pendant_serial_fault: serial_fault,
        pendant_quadrature_fault: quadrature_fault,
        pendant_sample,
        machine: state.task.machine,
        counts_by_motor: [
            unsafe { read(pins.motor_count[0]) },
            unsafe { read(pins.motor_count[1]) },
            unsafe { read(pins.motor_count[2]) },
        ],
        position_feedback_by_motor: [
            unsafe { read(pins.motor_position_feedback[0]) },
            unsafe { read(pins.motor_position_feedback[1]) },
            unsafe { read(pins.motor_position_feedback[2]) },
        ],
        raw_limits: [
            unsafe { read(pins.motor_limit_raw[0]) },
            unsafe { read(pins.motor_limit_raw[1]) },
            unsafe { read(pins.motor_limit_raw[2]) },
        ],
        safety_limits: [
            unsafe { read(pins.motor_limit_latched[0]) },
            unsafe { read(pins.motor_limit_latched[1]) },
            unsafe { read(pins.motor_limit_latched[2]) },
        ],
        pendant_mode_enabled: unsafe { read(pins.pendant_mode_enabled) },
        command_channel_ready,
    }
}

unsafe fn publish(pins: &Pins, outputs: RuntimeOutputs, sequencer: &HaluiCommandSequencer) {
    let supervisor = outputs.supervisor;
    unsafe {
        write(pins.heartbeat, outputs.heartbeat);
        write(pins.watchdog_enable, outputs.watchdog_enable);
        write(pins.external_enable, supervisor.external_enable);
        write(pins.position_known, outputs.position_known);
        write(pins.position_unknown, !outputs.position_known);
        write(pins.control_available, supervisor.control_available);
        write(pins.control_ready, supervisor.control_ready);
        write(pins.fault, supervisor.fault.is_some());
        write(
            pins.fault_code,
            supervisor.fault.map_or(0, |fault| fault as i32 + 1),
        );
        write(pins.recovery_active, supervisor.recovery_active);
        write(pins.jog_active, supervisor.jog_active);
        write(pins.bounce_active, supervisor.bounce_active);
        write(pins.supervisor_phase, supervisor.phase as i32);
        write(pins.mesa_phase, outputs.mesa_phase as i32);
        write(
            pins.controller_watchdog_phase,
            outputs.controller_watchdog_phase as i32,
        );
        write(pins.estop_reset_request, supervisor.estop_reset_request);
        write(pins.machine_on_request, supervisor.machine_on_request);
        for index in 0..3 {
            write(pins.motor_limit_reset[index], outputs.limit_reset[index]);
            write(
                pins.motor_command_enable[index],
                supervisor.command_enable_by_motor[index],
            );
            write(
                pins.motor_toward_limit[index],
                supervisor.toward_limit_by_motor[index],
            );
        }

        if outputs.mesa_watchdog_clear_requested {
            write(pins.mesa_watchdog_has_bit, false);
        }

        let command = sequencer.outputs(PULSES_PER_MM);
        write(pins.axis_jog_speed, command.axis_jog_speed);
        write(pins.joint_jog_speed, command.joint_jog_speed);
        write(pins.jog_stop, command.jog_stop);
        write(pins.jog_stop_immediate, command.jog_stop_immediate);
        write(pins.command_phase, command.phase as i32);
        for index in 0..3 {
            write(
                pins.axis_increment_plus[index],
                command.axis_increment_plus[index],
            );
            write(
                pins.axis_increment_minus[index],
                command.axis_increment_minus[index],
            );
            write(
                pins.joint_increment_plus[index],
                command.joint_increment_plus[index],
            );
            write(
                pins.joint_increment_minus[index],
                command.joint_increment_minus[index],
            );
            write(pins.axis_increment[index], command.axis_increment[index]);
            write(pins.joint_increment[index], command.joint_increment[index]);
        }
    }
}

unsafe fn publish_initial_safe(pins: &Pins) {
    unsafe {
        write(pins.serial_fault, true);
        write(pins.estop_pressed, true);
        write(pins.axis_code, AxisSelector::Invalid as i32);
        write(pins.multiplier_code, MultiplierSelector::Invalid as i32);
        write(pins.task_monitor_fault, true);
        write(pins.estopped, true);
        for index in 0..3 {
            write(pins.axis_stopped[index], true);
            write(pins.motor_limit_reset[index], false);
            write(pins.motor_command_enable[index], false);
            write(pins.motor_toward_limit[index], false);
            write(pins.axis_increment_plus[index], false);
            write(pins.axis_increment_minus[index], false);
            write(pins.joint_increment_plus[index], false);
            write(pins.joint_increment_minus[index], false);
            write(pins.axis_increment[index], 0.0);
            write(pins.joint_increment[index], 0.0);
        }
        write(pins.external_enable, false);
        write(pins.watchdog_enable, false);
        write(pins.heartbeat, false);
        write(pins.position_known, false);
        write(pins.position_unknown, true);
        write(pins.control_available, false);
        write(pins.control_ready, false);
        write(pins.fault, false);
        write(pins.fault_code, 0);
        write(pins.recovery_active, false);
        write(pins.jog_active, false);
        write(pins.bounce_active, false);
        write(pins.estop_reset_request, false);
        write(pins.machine_on_request, false);
        write(pins.axis_jog_speed, 0.0);
        write(pins.joint_jog_speed, 0.0);
        write(pins.jog_stop, false);
        write(pins.jog_stop_immediate, false);
    }
}

unsafe extern "C" fn update_component(argument: *mut c_void, period: c_long) {
    if argument.is_null() || period <= 0 {
        return;
    }
    let state = unsafe { &mut *argument.cast::<ComponentState>() };
    let pins = unsafe { &*state.pins };
    let period_ns = period as u64;
    state.sequencer.advance(period_ns);
    let inputs = unsafe { runtime_inputs(state, pins, state.sequencer.ready()) };
    let mut outputs = state.runtime.update(period_ns, inputs);
    if let Some(command) = outputs.supervisor.command {
        if !state.sequencer.accept(command) {
            state.runtime.fail(FaultCode::CommandSequencerFailure);
            let _ = state
                .sequencer
                .accept(dmc2_core::supervisor::CommandEvent::JogStopImmediate);
            outputs.supervisor = state.runtime.supervisor().outputs();
            outputs.limit_reset = outputs.supervisor.limit_reset;
        }
    }
    unsafe { publish(pins, outputs, &state.sequencer) };
}

#[no_mangle]
pub extern "C" fn rtapi_app_main() -> c_int {
    let component_id = unsafe { hal::hal_init(COMPONENT_NAME.as_ptr().cast::<c_char>()) };
    if component_id < 0 {
        return component_id;
    }
    unsafe { COMPONENT_ID = component_id };

    let result = (|| -> Result<(), c_int> {
        let pins = unsafe { hal::hal_malloc(mem::size_of::<Pins>() as c_long) }.cast::<Pins>();
        if pins.is_null() {
            return Err(ENOMEM);
        }
        unsafe { ptr::write_bytes(pins, 0, 1) };
        unsafe { register_pins(pins, component_id)? };
        unsafe { publish_initial_safe(&*pins) };

        let state = unsafe { hal::hal_malloc(mem::size_of::<ComponentState>() as c_long) }
            .cast::<ComponentState>();
        if state.is_null() {
            return Err(ENOMEM);
        }
        unsafe { ptr::write(state, ComponentState::new(pins)) };

        let exported = unsafe {
            hal::hal_export_funct(
                FUNCTION_NAME.as_ptr().cast::<c_char>(),
                Some(update_component),
                state.cast::<c_void>(),
                1,
                0,
                component_id,
            )
        };
        if exported != 0 {
            return Err(exported);
        }
        let ready = unsafe { hal::hal_ready(component_id) };
        if ready != 0 {
            return Err(ready);
        }
        Ok(())
    })();

    match result {
        Ok(()) => 0,
        Err(error) => {
            unsafe {
                hal::hal_exit(component_id);
                COMPONENT_ID = -1;
            }
            if error == 0 {
                EINVAL
            } else {
                error
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn rtapi_app_exit() {
    let component_id = unsafe { COMPONENT_ID };
    if component_id >= 0 {
        unsafe {
            hal::hal_exit(component_id);
            COMPONENT_ID = -1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::{String, ToString};
    use std::sync::Mutex;
    use std::vec::Vec;

    #[repr(align(16))]
    struct Arena([u8; 131_072]);

    static mut ARENA: Arena = Arena([0; 131_072]);
    static mut ARENA_OFFSET: usize = 0;
    static PINS: Mutex<Vec<(String, usize)>> = Mutex::new(Vec::new());
    static FUNCTION: Mutex<Option<(usize, usize)>> = Mutex::new(None);

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

    unsafe fn register<T>(name: *const c_char, pointer: *mut *mut T) -> c_int {
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
            .push((name, data as usize));
        0
    }

    #[no_mangle]
    extern "C" fn hal_init(name: *const c_char) -> c_int {
        let name = unsafe { std::ffi::CStr::from_ptr(name) }
            .to_str()
            .expect("component name was UTF-8");
        assert_eq!(name, "dmc2-pendant-control");
        41
    }

    #[no_mangle]
    extern "C" fn hal_exit(_component_id: c_int) -> c_int {
        0
    }

    #[no_mangle]
    extern "C" fn hal_ready(component_id: c_int) -> c_int {
        assert_eq!(component_id, 41);
        0
    }

    #[no_mangle]
    extern "C" fn hal_malloc(size: c_long) -> *mut c_void {
        if size <= 0 {
            return ptr::null_mut();
        }
        unsafe { allocate(size as usize) }
    }

    #[no_mangle]
    extern "C" fn hal_pin_bit_new(
        name: *const c_char,
        _direction: c_int,
        pointer: *mut *mut bool,
        _component_id: c_int,
    ) -> c_int {
        unsafe { register(name, pointer) }
    }

    #[no_mangle]
    extern "C" fn hal_pin_s32_new(
        name: *const c_char,
        _direction: c_int,
        pointer: *mut *mut i32,
        _component_id: c_int,
    ) -> c_int {
        unsafe { register(name, pointer) }
    }

    #[no_mangle]
    extern "C" fn hal_pin_u32_new(
        name: *const c_char,
        _direction: c_int,
        pointer: *mut *mut u32,
        _component_id: c_int,
    ) -> c_int {
        unsafe { register(name, pointer) }
    }

    #[no_mangle]
    extern "C" fn hal_pin_float_new(
        name: *const c_char,
        _direction: c_int,
        pointer: *mut *mut f64,
        _component_id: c_int,
    ) -> c_int {
        unsafe { register(name, pointer) }
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
        let function = function.expect("realtime function was present") as usize;
        *FUNCTION.lock().expect("mock function lock") = Some((function, argument as usize));
        0
    }

    fn pin<T>(name: &str) -> *mut T {
        let registry = PINS.lock().expect("mock HAL registry lock");
        registry
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, address)| *address as *mut T)
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
        let function: unsafe extern "C" fn(*mut c_void, c_long) =
            unsafe { mem::transmute(function) };
        unsafe { function(argument as *mut c_void, 1_000_000) };
    }

    fn publish_pendant(sequence: u32, detent: i32) {
        let generation = sequence.wrapping_shl(1);
        set_u32("dmc2-pendant-control.snapshot-generation", generation | 1);
        set_bit("dmc2-pendant-control.connected", true);
        set_bit("dmc2-pendant-control.serial-fault", false);
        set_bit("dmc2-pendant-control.quadrature-fault", false);
        set_bit("dmc2-pendant-control.estop-pressed", false);
        set_bit("dmc2-pendant-control.deadman-held", true);
        set_bit("dmc2-pendant-control.selector-valid", true);
        set_s32("dmc2-pendant-control.axis-code", 0);
        set_s32("dmc2-pendant-control.multiplier-code", 1);
        set_s32("dmc2-pendant-control.latest-detent", detent);
        set_u32("dmc2-pendant-control.quadrature-errors", 0);
        set_u32("dmc2-pendant-control.sequence", sequence);
        set_u32("dmc2-pendant-control.snapshot-generation", generation);
    }

    fn publish_task(heartbeat: u32) {
        let generation = heartbeat.wrapping_shl(1);
        set_u32(
            "dmc2-pendant-control.task-snapshot-generation",
            generation | 1,
        );
        set_bit("dmc2-pendant-control.task-monitor-connected", true);
        set_bit("dmc2-pendant-control.task-monitor-fault", false);
        set_u32("dmc2-pendant-control.task-heartbeat", heartbeat);
        set_bit("dmc2-pendant-control.machine-on", true);
        set_bit("dmc2-pendant-control.estopped", false);
        set_bit("dmc2-pendant-control.manual-mode", true);
        set_bit("dmc2-pendant-control.joint-mode", false);
        set_bit("dmc2-pendant-control.teleop-mode", true);
        set_bit("dmc2-pendant-control.interp-idle", true);
        for index in 0..3 {
            set_bit(&format!("dmc2-pendant-control.joint-{index}-homed"), true);
            set_bit(&format!("dmc2-pendant-control.joint-{index}-homing"), false);
            set_bit(&format!("dmc2-pendant-control.axis-{index}-stopped"), true);
        }
        set_u32("dmc2-pendant-control.task-snapshot-generation", generation);
    }

    #[test]
    fn exported_component_reaches_ready_and_emits_one_exact_x1_linuxcnc_edge() {
        unsafe {
            ARENA_OFFSET = 0;
            ptr::write_bytes(ptr::addr_of_mut!(ARENA.0).cast::<u8>(), 0, 131_072);
        }
        PINS.lock().expect("mock registry lock").clear();
        *FUNCTION.lock().expect("mock function lock") = None;
        assert_eq!(rtapi_app_main(), 0);

        set_bit("dmc2-pendant-control.servo-thread-ready", true);
        set_bit("dmc2-pendant-control.ui-ready", true);
        set_bit("dmc2-pendant-control.pendant-mode-enabled", true);
        set_bit("dmc2-pendant-control.mesa-watchdog-has-bit", false);
        set_bit("dmc2-pendant-control.mesa-packet-error-exceeded", false);

        let mut sequence = 1_u32;
        let mut heartbeat = 1_u32;
        for cycle in 0..700 {
            if cycle % 20 == 0 {
                sequence = sequence.wrapping_add(1);
                publish_pendant(sequence, 0);
            }
            heartbeat = heartbeat.wrapping_add(1);
            publish_task(heartbeat);
            set_bit(
                "dmc2-pendant-control.software-watchdog-ok",
                get_bit("dmc2-pendant-control.watchdog-enable"),
            );
            callback();
            if get_bit("dmc2-pendant-control.control-ready") {
                break;
            }
        }
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
            if cycle == 50 {
                set_s32("dmc2-pendant-control.motor-1-count", -10);
            }
            callback();
        }
        assert!(!get_bit("dmc2-pendant-control.axis-0-increment-minus"));
        assert!(!get_bit("dmc2-pendant-control.jog-active"));
        assert!(!get_bit("dmc2-pendant-control.fault"));
        assert_eq!(get_s32("dmc2-pendant-control.fault-code"), 0);
        rtapi_app_exit();
    }
}
