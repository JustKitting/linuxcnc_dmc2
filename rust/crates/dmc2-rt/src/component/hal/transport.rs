use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

use dmc2_core::motion::{MotionCommandPhase, NativeMotionCommandChannel};
use dmc2_core::pendant::{AxisSelector, MultiplierSelector, PendantSample};
use dmc2_core::runtime::{RuntimeInputs, RuntimeOutputs};
use dmc2_core::startup::{ControllerWatchdogPhase, MesaStartupPhase};
use dmc2_core::supervisor::{FaultCode, FaultRecord, MachineSnapshot, MotionSnapshot, Phase};
use dmc2_hal_sys as hal;

use super::Pins;
use crate::component::state::{CachedTaskSnapshot, ComponentState};

unsafe fn read<T: Copy>(pointer: *mut T) -> T {
    unsafe { ptr::read_volatile(pointer) }
}

unsafe fn write<T: Copy>(pointer: *mut T, value: T) {
    unsafe { ptr::write_volatile(pointer, value) }
}

const fn seconds(nanoseconds: u64) -> f64 {
    nanoseconds as f64 / 1_000_000_000.0
}

unsafe fn publish_fault_record(pins: &Pins, record: Option<FaultRecord>) {
    let generation_pin = unsafe { &*(pins.fault_snapshot_generation.cast::<AtomicU32>()) };
    let current_generation = generation_pin.load(Ordering::SeqCst) & !1;
    let updating_generation = current_generation.wrapping_add(1);
    let complete_generation = current_generation.wrapping_add(2);
    generation_pin.store(updating_generation, Ordering::SeqCst);
    let code = record.map(|value| value.code);
    unsafe {
        write(pins.fault, record.is_some());
        write(pins.fault_code, code.map_or(0, FaultCode::wire_code));
        for (index, known) in FaultCode::ALL.iter().copied().enumerate() {
            write(pins.fault_kind[index], code == Some(known));
        }

        write(pins.fault_evidence_valid, record.is_some());
        let Some(record) = record else {
            clear_fault_evidence(pins);
            generation_pin.store(complete_generation, Ordering::SeqCst);
            return;
        };
        let evidence = record.evidence;
        write(pins.fault_axis_valid, evidence.axis.is_some());
        for index in 0..3 {
            write(
                pins.fault_axis[index],
                evidence.axis.is_some_and(|axis| axis.index() == index),
            );
        }
        let valid_motor = evidence.motor.filter(|motor| *motor <= i32::MAX as usize);
        write(pins.fault_motor_valid, valid_motor.is_some());
        write(
            pins.fault_motor,
            valid_motor.map_or(0, |motor| motor as i32),
        );
        write_optional_s32(
            pins.fault_start_count_valid,
            pins.fault_start_count,
            evidence.start_count,
        );
        write_optional_s32(
            pins.fault_target_count_valid,
            pins.fault_target_count,
            evidence.target_count,
        );
        write_optional_s32(
            pins.fault_observed_count_valid,
            pins.fault_observed_count,
            evidence.observed_count,
        );
        write_optional_float(
            pins.fault_target_position_valid,
            pins.fault_target_position_pulses,
            evidence.target_position_pulses,
        );
        write_optional_float(
            pins.fault_observed_position_valid,
            pins.fault_observed_position_pulses,
            evidence.observed_position_pulses,
        );
        write_optional_float(
            pins.fault_position_error_valid,
            pins.fault_position_error_pulses,
            evidence.position_error_pulses,
        );
        for index in 0..3 {
            write(
                pins.fault_counts_by_motor[index],
                evidence.counts_by_motor[index],
            );
            write(
                pins.fault_position_feedback_by_motor[index],
                evidence.position_feedback_by_motor[index],
            );
        }
        write(pins.fault_raw_limit_mask, evidence.raw_limit_mask);
        write(pins.fault_safety_limit_mask, evidence.safety_limit_mask);
        write_optional_u32(
            pins.fault_expected_limit_mask_valid,
            pins.fault_expected_limit_mask,
            evidence.expected_limit_mask,
        );
        write_optional_float(
            pins.fault_elapsed_valid,
            pins.fault_elapsed_seconds,
            evidence.elapsed_ns.map(seconds),
        );
        write_optional_float(
            pins.fault_timeout_valid,
            pins.fault_timeout_seconds,
            evidence.timeout_ns.map(seconds),
        );
        write(pins.fault_link_connected, evidence.link_connected);
        write(pins.fault_serial_fault, evidence.serial_fault);
        write(pins.fault_quadrature_fault, evidence.quadrature_fault);
        write(
            pins.fault_pendant_estop_pressed,
            evidence.pendant_estop_pressed,
        );
        write(pins.fault_machine_on, evidence.machine_on);
        write(pins.fault_machine_estopped, evidence.machine_estopped);
        write(pins.fault_manual_mode, evidence.manual_mode);
        write(pins.fault_joint_mode, evidence.joint_mode);
        write(pins.fault_teleop_mode, evidence.teleop_mode);
        write(pins.fault_interp_idle, evidence.interp_idle);
        write(pins.fault_homed_mask, evidence.homed_mask);
        write(pins.fault_homing_mask, evidence.homing_mask);
        write(pins.fault_stopped_mask, evidence.stopped_mask);
        write(
            pins.fault_motion_command_ready,
            evidence.motion_command_ready,
        );
        write(pins.fault_motion_enabled, evidence.motion_enabled);
        write(pins.fault_motion_teleop_mode, evidence.motion_teleop_mode);
        write(pins.fault_motion_coord_mode, evidence.motion_coord_mode);
        write(pins.fault_motion_in_position, evidence.motion_in_position);
        write(pins.fault_motion_jog_active, evidence.motion_jog_active);
        write(
            pins.fault_consumer_active_seen,
            evidence.consumer_active_seen,
        );
        write(
            pins.fault_feedback_progress_seen,
            evidence.feedback_progress_seen,
        );
        write(
            pins.fault_axis_wheel_jog_active_mask,
            evidence.axis_wheel_jog_active_mask,
        );
        write(
            pins.fault_joint_wheel_jog_active_mask,
            evidence.joint_wheel_jog_active_mask,
        );
        write(
            pins.fault_joint_in_position_mask,
            evidence.joint_in_position_mask,
        );
        write_optional_float(
            pins.fault_task_heartbeat_age_valid,
            pins.fault_task_heartbeat_age_seconds,
            evidence.task_heartbeat_age_ns.map(seconds),
        );
        write_optional_float(
            pins.fault_pendant_packet_age_valid,
            pins.fault_pendant_packet_age_seconds,
            evidence.pendant_packet_age_ns.map(seconds),
        );
        write(pins.fault_supervisor_phase, evidence.supervisor_phase);
        let supervisor_phase = Phase::from_wire_code(evidence.supervisor_phase);
        for (index, known) in Phase::ALL.iter().copied().enumerate() {
            write(
                pins.fault_supervisor_phase_kind[index],
                supervisor_phase == Some(known),
            );
        }
        write_optional_s32(
            pins.fault_mesa_phase_valid,
            pins.fault_mesa_phase,
            evidence.mesa_phase,
        );
        let mesa_phase = evidence
            .mesa_phase
            .and_then(MesaStartupPhase::from_wire_code);
        for (index, known) in MesaStartupPhase::ALL.iter().copied().enumerate() {
            write(pins.fault_mesa_phase_kind[index], mesa_phase == Some(known));
        }
        write_optional_s32(
            pins.fault_controller_watchdog_phase_valid,
            pins.fault_controller_watchdog_phase,
            evidence.controller_watchdog_phase,
        );
        let watchdog_phase = evidence
            .controller_watchdog_phase
            .and_then(ControllerWatchdogPhase::from_wire_code);
        for (index, known) in ControllerWatchdogPhase::ALL.iter().copied().enumerate() {
            write(
                pins.fault_controller_watchdog_phase_kind[index],
                watchdog_phase == Some(known),
            );
        }
        generation_pin.store(complete_generation, Ordering::SeqCst);
    }
}

unsafe fn clear_fault_evidence(pins: &Pins) {
    unsafe {
        for pointer in [
            pins.fault_axis_valid,
            pins.fault_motor_valid,
            pins.fault_start_count_valid,
            pins.fault_target_count_valid,
            pins.fault_observed_count_valid,
            pins.fault_target_position_valid,
            pins.fault_observed_position_valid,
            pins.fault_position_error_valid,
            pins.fault_expected_limit_mask_valid,
            pins.fault_elapsed_valid,
            pins.fault_timeout_valid,
            pins.fault_link_connected,
            pins.fault_serial_fault,
            pins.fault_quadrature_fault,
            pins.fault_pendant_estop_pressed,
            pins.fault_machine_on,
            pins.fault_machine_estopped,
            pins.fault_manual_mode,
            pins.fault_joint_mode,
            pins.fault_teleop_mode,
            pins.fault_interp_idle,
            pins.fault_motion_command_ready,
            pins.fault_motion_enabled,
            pins.fault_motion_teleop_mode,
            pins.fault_motion_coord_mode,
            pins.fault_motion_in_position,
            pins.fault_motion_jog_active,
            pins.fault_consumer_active_seen,
            pins.fault_feedback_progress_seen,
            pins.fault_task_heartbeat_age_valid,
            pins.fault_pendant_packet_age_valid,
            pins.fault_mesa_phase_valid,
            pins.fault_controller_watchdog_phase_valid,
        ] {
            write(pointer, false);
        }
        for pointer in pins.fault_axis {
            write(pointer, false);
        }
        for pointer in pins.fault_supervisor_phase_kind {
            write(pointer, false);
        }
        for pointer in pins.fault_mesa_phase_kind {
            write(pointer, false);
        }
        for pointer in pins.fault_controller_watchdog_phase_kind {
            write(pointer, false);
        }
        for pointer in [
            pins.fault_motor,
            pins.fault_start_count,
            pins.fault_target_count,
            pins.fault_observed_count,
            pins.fault_supervisor_phase,
            pins.fault_mesa_phase,
            pins.fault_controller_watchdog_phase,
        ] {
            write(pointer, 0);
        }
        for pointer in pins.fault_counts_by_motor {
            write(pointer, 0);
        }
        for pointer in [
            pins.fault_target_position_pulses,
            pins.fault_observed_position_pulses,
            pins.fault_position_error_pulses,
            pins.fault_elapsed_seconds,
            pins.fault_timeout_seconds,
            pins.fault_task_heartbeat_age_seconds,
            pins.fault_pendant_packet_age_seconds,
        ] {
            write(pointer, 0.0);
        }
        for pointer in pins.fault_position_feedback_by_motor {
            write(pointer, 0.0);
        }
        for pointer in [
            pins.fault_raw_limit_mask,
            pins.fault_safety_limit_mask,
            pins.fault_expected_limit_mask,
            pins.fault_homed_mask,
            pins.fault_homing_mask,
            pins.fault_stopped_mask,
            pins.fault_axis_wheel_jog_active_mask,
            pins.fault_joint_wheel_jog_active_mask,
            pins.fault_joint_in_position_mask,
        ] {
            write(pointer, 0);
        }
    }
}

unsafe fn write_optional_s32(
    valid: *mut hal::hal_bit_t,
    value: *mut hal::hal_s32_t,
    source: Option<i32>,
) {
    unsafe {
        write(valid, source.is_some());
        write(value, source.unwrap_or(0));
    }
}

unsafe fn write_optional_u32(
    valid: *mut hal::hal_bit_t,
    value: *mut hal::hal_u32_t,
    source: Option<u32>,
) {
    unsafe {
        write(valid, source.is_some());
        write(value, source.unwrap_or(0));
    }
}

unsafe fn write_optional_float(
    valid: *mut hal::hal_bit_t,
    value: *mut hal::real_t,
    source: Option<f64>,
) {
    unsafe {
        write(valid, source.is_some());
        write(value, source.unwrap_or(0.0));
    }
}

unsafe fn generation(pointer: *mut hal::hal_u32_t) -> u32 {
    unsafe { (*pointer.cast::<AtomicU32>()).load(Ordering::SeqCst) }
}

const fn generation_is_coherent(first: u32, second: u32) -> bool {
    first == second && second & 1 == 0
}

struct PendantSnapshot {
    coherent: bool,
    sample: PendantSample,
    connected: bool,
    serial_fault: bool,
    quadrature_fault: bool,
    fault_reset_ack: u32,
}

unsafe fn read_pendant(pins: &Pins) -> PendantSnapshot {
    let first = unsafe { generation(pins.pendant_snapshot_generation) };
    if first & 1 != 0 {
        return PendantSnapshot {
            coherent: false,
            sample: safe_pendant(),
            connected: false,
            serial_fault: true,
            quadrature_fault: false,
            fault_reset_ack: 0,
        };
    }
    let connected = unsafe { read(pins.connected) };
    let serial_fault = unsafe { read(pins.serial_fault) };
    let quadrature_fault = unsafe { read(pins.quadrature_fault) };
    let fault_reset_ack = unsafe { read(pins.pendant_fault_reset_ack) };
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
    PendantSnapshot {
        coherent: generation_is_coherent(first, second),
        sample,
        connected,
        serial_fault,
        quadrature_fault,
        fault_reset_ack,
    }
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

fn commit_task_snapshot(
    cached: &mut CachedTaskSnapshot,
    first: u32,
    second: u32,
    value: CachedTaskSnapshot,
) {
    if generation_is_coherent(first, second) {
        *cached = value;
    }
}

unsafe fn refresh_task_snapshot(pins: &Pins, cached: &mut CachedTaskSnapshot) {
    let first = unsafe { generation(pins.task_snapshot_generation) };
    if first & 1 != 0 {
        return;
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
    commit_task_snapshot(cached, first, second, value);
}

pub(in crate::component) unsafe fn runtime_inputs(
    state: &mut ComponentState,
    pins: &Pins,
    motion_command_ready: bool,
) -> RuntimeInputs {
    let pendant = unsafe { read_pendant(pins) };
    unsafe { refresh_task_snapshot(pins, &mut state.task) };
    RuntimeInputs {
        servo_thread_ready: unsafe { read(pins.servo_thread_ready) },
        mesa_watchdog_has_bit: unsafe { read(pins.mesa_watchdog_has_bit) },
        mesa_io_error: unsafe { read(pins.mesa_packet_error_exceeded) },
        software_watchdog_ok: unsafe { read(pins.software_watchdog_ok) },
        ui_ready: unsafe { read(pins.ui_ready) },
        task_monitor_connected: state.task.connected,
        task_monitor_fault: state.task.fault,
        task_heartbeat: state.task.heartbeat,
        pendant_coherent: pendant.coherent,
        pendant_connected: pendant.connected,
        pendant_serial_fault: pendant.serial_fault,
        pendant_quadrature_fault: pendant.quadrature_fault,
        pendant_fault_reset_ack: pendant.fault_reset_ack,
        pendant_sample: pendant.sample,
        machine: state.task.machine,
        motion: MotionSnapshot {
            enabled: unsafe { read(pins.motion_enabled) },
            teleop_mode: unsafe { read(pins.motion_teleop_mode) },
            coord_mode: unsafe { read(pins.motion_coord_mode) },
            in_position: unsafe { read(pins.motion_in_position) },
            jog_active: unsafe { read(pins.motion_jog_active) },
            axis_wheel_jog_active: [
                unsafe { read(pins.axis_wheel_jog_active[0]) },
                unsafe { read(pins.axis_wheel_jog_active[1]) },
                unsafe { read(pins.axis_wheel_jog_active[2]) },
            ],
            joint_wheel_jog_active: [
                unsafe { read(pins.joint_wheel_jog_active[0]) },
                unsafe { read(pins.joint_wheel_jog_active[1]) },
                unsafe { read(pins.joint_wheel_jog_active[2]) },
            ],
            joint_in_position: [
                unsafe { read(pins.joint_in_position[0]) },
                unsafe { read(pins.joint_in_position[1]) },
                unsafe { read(pins.joint_in_position[2]) },
            ],
        },
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
        motion_command_ready,
        linuxcnc_estop_reset_request: unsafe { read(pins.linuxcnc_estop_reset_request) },
    }
}

pub(in crate::component) unsafe fn publish(
    pins: &Pins,
    outputs: RuntimeOutputs,
    motion_commands: &NativeMotionCommandChannel,
) {
    let supervisor = outputs.supervisor;
    unsafe {
        write(pins.heartbeat, outputs.heartbeat);
        write(pins.watchdog_enable, outputs.watchdog_enable);
        write(pins.external_enable, supervisor.external_enable);
        write(pins.position_known, outputs.position_known);
        write(pins.position_unknown, !outputs.position_known);
        write(pins.control_ready, supervisor.control_ready);
        write(pins.fault_reset_allowed, outputs.fault_reset_allowed);
        write(
            pins.pendant_fault_reset_request,
            outputs.pendant_fault_reset_request,
        );
        publish_fault_record(pins, supervisor.fault_record);
        write(pins.recovery_active, supervisor.recovery_active);
        write(pins.jog_active, supervisor.jog_active);
        write(pins.bounce_active, supervisor.bounce_active);
        write(pins.supervisor_phase, supervisor.phase.wire_code());
        for (index, known) in Phase::ALL.iter().copied().enumerate() {
            write(pins.supervisor_phase_kind[index], supervisor.phase == known);
        }
        write(pins.mesa_phase, outputs.mesa_phase.wire_code());
        for (index, known) in MesaStartupPhase::ALL.iter().copied().enumerate() {
            write(pins.mesa_phase_kind[index], outputs.mesa_phase == known);
        }
        write(
            pins.controller_watchdog_phase,
            outputs.controller_watchdog_phase.wire_code(),
        );
        for (index, known) in ControllerWatchdogPhase::ALL.iter().copied().enumerate() {
            write(
                pins.controller_watchdog_phase_kind[index],
                outputs.controller_watchdog_phase == known,
            );
        }
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

        let command = motion_commands.outputs();
        write(pins.jog_stop, command.jog_stop);
        write(pins.jog_stop_immediate, command.jog_stop_immediate);
        write(pins.command_phase, command.phase.wire_code());
        for (index, known) in MotionCommandPhase::ALL.iter().copied().enumerate() {
            write(pins.command_phase_kind[index], command.phase == known);
        }
        for index in 0..3 {
            write(pins.axis_jog_counts[index], command.axis_jog_counts[index]);
            write(
                pins.joint_jog_counts[index],
                command.joint_jog_counts[index],
            );
            write(pins.axis_jog_scale[index], command.axis_jog_scale[index]);
            write(pins.joint_jog_scale[index], command.joint_jog_scale[index]);
            write(pins.axis_jog_enable[index], command.axis_jog_enable[index]);
            write(
                pins.joint_jog_enable[index],
                command.joint_jog_enable[index],
            );
            write(
                pins.axis_jog_vel_mode[index],
                command.axis_jog_vel_mode[index],
            );
            write(
                pins.joint_jog_vel_mode[index],
                command.joint_jog_vel_mode[index],
            );
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
            write(pins.axis_wheel_jog_active[index], false);
            write(pins.joint_wheel_jog_active[index], false);
            write(pins.joint_in_position[index], false);
            write(pins.motor_limit_reset[index], false);
            write(pins.motor_command_enable[index], false);
            write(pins.motor_toward_limit[index], false);
            write(pins.axis_jog_counts[index], 0);
            write(pins.joint_jog_counts[index], 0);
            write(pins.axis_jog_scale[index], 0.0);
            write(pins.joint_jog_scale[index], 0.0);
            write(pins.axis_jog_enable[index], false);
            write(pins.joint_jog_enable[index], false);
            write(pins.axis_jog_vel_mode[index], false);
            write(pins.joint_jog_vel_mode[index], false);
        }
        write(pins.motion_enabled, false);
        write(pins.motion_teleop_mode, false);
        write(pins.motion_coord_mode, false);
        write(pins.motion_in_position, false);
        write(pins.motion_jog_active, false);
        write(pins.external_enable, false);
        write(pins.watchdog_enable, false);
        write(pins.heartbeat, false);
        write(pins.position_known, false);
        write(pins.position_unknown, true);
        write(pins.control_ready, false);
        write(pins.fault_reset_allowed, false);
        write(pins.pendant_fault_reset_request, 0);
        write(pins.pendant_fault_reset_ack, 0);
        write(pins.fault_snapshot_generation, 0);
        publish_fault_record(pins, None);
        write(pins.recovery_active, false);
        write(pins.jog_active, false);
        write(pins.bounce_active, false);
        write(pins.supervisor_phase, Phase::Idle.wire_code());
        for (index, known) in Phase::ALL.iter().copied().enumerate() {
            write(pins.supervisor_phase_kind[index], known == Phase::Idle);
        }
        write(pins.mesa_phase, MesaStartupPhase::WaitServo.wire_code());
        for (index, known) in MesaStartupPhase::ALL.iter().copied().enumerate() {
            write(
                pins.mesa_phase_kind[index],
                known == MesaStartupPhase::WaitServo,
            );
        }
        write(
            pins.controller_watchdog_phase,
            ControllerWatchdogPhase::WaitPrerequisites.wire_code(),
        );
        for (index, known) in ControllerWatchdogPhase::ALL.iter().copied().enumerate() {
            write(
                pins.controller_watchdog_phase_kind[index],
                known == ControllerWatchdogPhase::WaitPrerequisites,
            );
        }
        write(pins.estop_reset_request, false);
        write(pins.machine_on_request, false);
        write(pins.jog_stop, false);
        write(pins.jog_stop_immediate, false);
        write(pins.command_phase, MotionCommandPhase::Idle.wire_code());
        for (index, known) in MotionCommandPhase::ALL.iter().copied().enumerate() {
            write(
                pins.command_phase_kind[index],
                known == MotionCommandPhase::Idle,
            );
        }
    }
}

pub(in crate::component) unsafe fn manual_probe_contact(pins: &Pins) -> bool {
    unsafe { read(pins.manual_probe_contact) }
}
