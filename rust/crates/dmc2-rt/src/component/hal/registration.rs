use core::ffi::{c_char, c_int};
use core::ptr;

use dmc2_hal_sys as hal;

use super::Pins;

unsafe fn bit_pin(
    pointer: *mut *mut hal::hal_bit_t,
    name: &'static [u8],
    direction: hal::hal_pin_dir_t,
    component_id: c_int,
) -> Result<(), hal::HalError> {
    let result = unsafe {
        hal::hal_pin_bit_new(
            name.as_ptr().cast::<c_char>(),
            direction,
            pointer,
            component_id,
        )
    };
    hal::HalCall::PinBitNew.classify(result).map(|_| ())
}

unsafe fn s32_pin(
    pointer: *mut *mut hal::hal_s32_t,
    name: &'static [u8],
    direction: hal::hal_pin_dir_t,
    component_id: c_int,
) -> Result<(), hal::HalError> {
    let result = unsafe {
        hal::hal_pin_s32_new(
            name.as_ptr().cast::<c_char>(),
            direction,
            pointer,
            component_id,
        )
    };
    hal::HalCall::PinS32New.classify(result).map(|_| ())
}

unsafe fn u32_pin(
    pointer: *mut *mut hal::hal_u32_t,
    name: &'static [u8],
    direction: hal::hal_pin_dir_t,
    component_id: c_int,
) -> Result<(), hal::HalError> {
    let result = unsafe {
        hal::hal_pin_u32_new(
            name.as_ptr().cast::<c_char>(),
            direction,
            pointer,
            component_id,
        )
    };
    hal::HalCall::PinU32New.classify(result).map(|_| ())
}

unsafe fn float_pin(
    pointer: *mut *mut hal::real_t,
    name: &'static [u8],
    direction: hal::hal_pin_dir_t,
    component_id: c_int,
) -> Result<(), hal::HalError> {
    let result = unsafe {
        hal::hal_pin_float_new(
            name.as_ptr().cast::<c_char>(),
            direction,
            pointer,
            component_id,
        )
    };
    hal::HalCall::PinFloatNew.classify(result).map(|_| ())
}

macro_rules! scalar_pins {
    ($function:ident, $pins:ident, $direction:ident, $component_id:ident; $($field:ident => $name:literal),+ $(,)?) => {
        $(unsafe {
            $function(
                ptr::addr_of_mut!((*$pins).$field),
                concat!($name, "\0").as_bytes(),
                $direction,
                $component_id,
            )?;
        })+
    };
}

macro_rules! indexed_pins {
    ($function:ident, $pins:ident, $direction:ident, $component_id:ident, $field:ident; $($index:literal => $name:literal),+ $(,)?) => {
        $(unsafe {
            $function(
                ptr::addr_of_mut!((*$pins).$field[$index]),
                concat!($name, "\0").as_bytes(),
                $direction,
                $component_id,
            )?;
        })+
    };
}

pub(in crate::component) unsafe fn register_pins(
    pins: *mut Pins,
    component_id: c_int,
) -> Result<(), hal::HalError> {
    let input = hal::hal_pin_dir_t_HAL_IN;
    let output = hal::hal_pin_dir_t_HAL_OUT;
    let io = hal::hal_pin_dir_t_HAL_IO;

    scalar_pins!(u32_pin, pins, input, component_id;
        pendant_snapshot_generation => "dmc2-pendant-control.snapshot-generation",
    );
    scalar_pins!(bit_pin, pins, input, component_id;
        connected => "dmc2-pendant-control.connected",
        serial_fault => "dmc2-pendant-control.serial-fault",
        quadrature_fault => "dmc2-pendant-control.quadrature-fault",
        estop_pressed => "dmc2-pendant-control.estop-pressed",
        deadman_held => "dmc2-pendant-control.deadman-held",
        selector_valid => "dmc2-pendant-control.selector-valid",
    );
    scalar_pins!(s32_pin, pins, input, component_id;
        axis_code => "dmc2-pendant-control.axis-code",
        multiplier_code => "dmc2-pendant-control.multiplier-code",
        latest_detent => "dmc2-pendant-control.latest-detent",
        detent_count => "dmc2-pendant-control.detent-count",
        transition_count => "dmc2-pendant-control.transition-count",
    );
    scalar_pins!(u32_pin, pins, input, component_id;
        quadrature_errors => "dmc2-pendant-control.quadrature-errors",
        sequence => "dmc2-pendant-control.sequence",
        milliseconds => "dmc2-pendant-control.milliseconds",
        task_snapshot_generation => "dmc2-pendant-control.task-snapshot-generation",
    );
    scalar_pins!(bit_pin, pins, input, component_id;
        task_monitor_connected => "dmc2-pendant-control.task-monitor-connected",
        task_monitor_fault => "dmc2-pendant-control.task-monitor-fault",
    );
    scalar_pins!(u32_pin, pins, input, component_id;
        task_heartbeat => "dmc2-pendant-control.task-heartbeat",
    );
    scalar_pins!(bit_pin, pins, input, component_id;
        machine_on => "dmc2-pendant-control.machine-on",
        estopped => "dmc2-pendant-control.estopped",
        manual_mode => "dmc2-pendant-control.manual-mode",
        joint_mode => "dmc2-pendant-control.joint-mode",
        teleop_mode => "dmc2-pendant-control.teleop-mode",
        interp_idle => "dmc2-pendant-control.interp-idle",
    );
    indexed_pins!(bit_pin, pins, input, component_id, joint_homed;
        0 => "dmc2-pendant-control.joint-0-homed",
        1 => "dmc2-pendant-control.joint-1-homed",
        2 => "dmc2-pendant-control.joint-2-homed",
    );
    indexed_pins!(bit_pin, pins, input, component_id, joint_homing;
        0 => "dmc2-pendant-control.joint-0-homing",
        1 => "dmc2-pendant-control.joint-1-homing",
        2 => "dmc2-pendant-control.joint-2-homing",
    );
    indexed_pins!(bit_pin, pins, input, component_id, axis_stopped;
        0 => "dmc2-pendant-control.axis-0-stopped",
        1 => "dmc2-pendant-control.axis-1-stopped",
        2 => "dmc2-pendant-control.axis-2-stopped",
    );
    indexed_pins!(s32_pin, pins, input, component_id, motor_count;
        0 => "dmc2-pendant-control.motor-0-count",
        1 => "dmc2-pendant-control.motor-1-count",
        2 => "dmc2-pendant-control.motor-2-count",
    );
    indexed_pins!(float_pin, pins, input, component_id, motor_position_feedback;
        0 => "dmc2-pendant-control.motor-0-position-feedback",
        1 => "dmc2-pendant-control.motor-1-position-feedback",
        2 => "dmc2-pendant-control.motor-2-position-feedback",
    );
    indexed_pins!(bit_pin, pins, input, component_id, motor_limit_raw;
        0 => "dmc2-pendant-control.motor-0-limit-raw",
        1 => "dmc2-pendant-control.motor-1-limit-raw",
        2 => "dmc2-pendant-control.motor-2-limit-raw",
    );
    indexed_pins!(bit_pin, pins, input, component_id, motor_limit_latched;
        0 => "dmc2-pendant-control.motor-0-limit-latched",
        1 => "dmc2-pendant-control.motor-1-limit-latched",
        2 => "dmc2-pendant-control.motor-2-limit-latched",
    );
    scalar_pins!(bit_pin, pins, input, component_id;
        pendant_mode_enabled => "dmc2-pendant-control.pendant-mode-enabled",
        servo_thread_ready => "dmc2-pendant-control.servo-thread-ready",
    );
    scalar_pins!(bit_pin, pins, io, component_id;
        mesa_watchdog_has_bit => "dmc2-pendant-control.mesa-watchdog-has-bit",
    );
    scalar_pins!(bit_pin, pins, input, component_id;
        mesa_packet_error => "dmc2-pendant-control.mesa-packet-error",
    );
    scalar_pins!(u32_pin, pins, input, component_id;
        mesa_packet_error_total => "dmc2-pendant-control.mesa-packet-error-total",
    );
    scalar_pins!(bit_pin, pins, input, component_id;
        mesa_packet_error_exceeded => "dmc2-pendant-control.mesa-packet-error-exceeded",
        software_watchdog_ok => "dmc2-pendant-control.software-watchdog-ok",
        ui_ready => "dmc2-pendant-control.ui-ready",
    );

    indexed_pins!(bit_pin, pins, output, component_id, motor_limit_reset;
        0 => "dmc2-pendant-control.motor-0-limit-reset",
        1 => "dmc2-pendant-control.motor-1-limit-reset",
        2 => "dmc2-pendant-control.motor-2-limit-reset",
    );
    indexed_pins!(bit_pin, pins, output, component_id, motor_command_enable;
        0 => "dmc2-pendant-control.motor-0-command-enable",
        1 => "dmc2-pendant-control.motor-1-command-enable",
        2 => "dmc2-pendant-control.motor-2-command-enable",
    );
    indexed_pins!(bit_pin, pins, output, component_id, motor_toward_limit;
        0 => "dmc2-pendant-control.motor-0-toward-limit",
        1 => "dmc2-pendant-control.motor-1-toward-limit",
        2 => "dmc2-pendant-control.motor-2-toward-limit",
    );
    scalar_pins!(bit_pin, pins, output, component_id;
        external_enable => "dmc2-pendant-control.external-enable",
        watchdog_enable => "dmc2-pendant-control.watchdog-enable",
        heartbeat => "dmc2-pendant-control.heartbeat",
        position_known => "dmc2-pendant-control.position-known",
        position_unknown => "dmc2-pendant-control.position-unknown",
        control_available => "dmc2-pendant-control.control-available",
        control_ready => "dmc2-pendant-control.control-ready",
        fault => "dmc2-pendant-control.fault",
    );
    scalar_pins!(s32_pin, pins, output, component_id;
        fault_code => "dmc2-pendant-control.fault-code",
    );
    scalar_pins!(bit_pin, pins, output, component_id;
        recovery_active => "dmc2-pendant-control.recovery-active",
        jog_active => "dmc2-pendant-control.jog-active",
        bounce_active => "dmc2-pendant-control.bounce-active",
    );
    scalar_pins!(s32_pin, pins, output, component_id;
        supervisor_phase => "dmc2-pendant-control.supervisor-phase",
        mesa_phase => "dmc2-pendant-control.mesa-phase",
        controller_watchdog_phase => "dmc2-pendant-control.controller-watchdog-phase",
    );
    scalar_pins!(bit_pin, pins, output, component_id;
        estop_reset_request => "dmc2-pendant-control.estop-reset-request",
        machine_on_request => "dmc2-pendant-control.machine-on-request",
    );
    indexed_pins!(bit_pin, pins, output, component_id, axis_increment_plus;
        0 => "dmc2-pendant-control.axis-0-increment-plus",
        1 => "dmc2-pendant-control.axis-1-increment-plus",
        2 => "dmc2-pendant-control.axis-2-increment-plus",
    );
    indexed_pins!(bit_pin, pins, output, component_id, axis_increment_minus;
        0 => "dmc2-pendant-control.axis-0-increment-minus",
        1 => "dmc2-pendant-control.axis-1-increment-minus",
        2 => "dmc2-pendant-control.axis-2-increment-minus",
    );
    indexed_pins!(bit_pin, pins, output, component_id, joint_increment_plus;
        0 => "dmc2-pendant-control.joint-0-increment-plus",
        1 => "dmc2-pendant-control.joint-1-increment-plus",
        2 => "dmc2-pendant-control.joint-2-increment-plus",
    );
    indexed_pins!(bit_pin, pins, output, component_id, joint_increment_minus;
        0 => "dmc2-pendant-control.joint-0-increment-minus",
        1 => "dmc2-pendant-control.joint-1-increment-minus",
        2 => "dmc2-pendant-control.joint-2-increment-minus",
    );
    indexed_pins!(float_pin, pins, output, component_id, axis_increment;
        0 => "dmc2-pendant-control.axis-0-increment",
        1 => "dmc2-pendant-control.axis-1-increment",
        2 => "dmc2-pendant-control.axis-2-increment",
    );
    indexed_pins!(float_pin, pins, output, component_id, joint_increment;
        0 => "dmc2-pendant-control.joint-0-increment",
        1 => "dmc2-pendant-control.joint-1-increment",
        2 => "dmc2-pendant-control.joint-2-increment",
    );
    scalar_pins!(float_pin, pins, output, component_id;
        axis_jog_speed => "dmc2-pendant-control.axis-jog-speed",
        joint_jog_speed => "dmc2-pendant-control.joint-jog-speed",
    );
    scalar_pins!(bit_pin, pins, output, component_id;
        jog_stop => "dmc2-pendant-control.jog-stop",
        jog_stop_immediate => "dmc2-pendant-control.jog-stop-immediate",
    );
    scalar_pins!(s32_pin, pins, output, component_id;
        command_phase => "dmc2-pendant-control.command-phase",
    );
    Ok(())
}
