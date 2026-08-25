use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

use dmc2_core::halui::HaluiCommandSequencer;
use dmc2_core::pendant::{AxisSelector, MultiplierSelector, PendantSample};
use dmc2_core::runtime::{RuntimeInputs, RuntimeOutputs};
use dmc2_core::supervisor::{FaultCode, MachineSnapshot};
use dmc2_core::PULSES_PER_MM;
use dmc2_hal_sys as hal;

use super::Pins;
use crate::component::state::{CachedTaskSnapshot, ComponentState};

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

pub(in crate::component) unsafe fn runtime_inputs(
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

pub(in crate::component) unsafe fn publish(
    pins: &Pins,
    outputs: RuntimeOutputs,
    sequencer: &HaluiCommandSequencer,
) {
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
            supervisor.fault.map_or(0, FaultCode::wire_code),
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

pub(in crate::component) unsafe fn publish_initial_safe(pins: &Pins) {
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
