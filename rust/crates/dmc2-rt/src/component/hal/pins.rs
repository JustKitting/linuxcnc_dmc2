use dmc2_core::motion::MotionCommandPhase;
use dmc2_core::startup::{ControllerWatchdogPhase, MesaStartupPhase};
use dmc2_core::supervisor::{FaultCode, Phase};
use dmc2_hal_sys as hal;

hal::realtime_hal_pin_catalog! {
    pub(in crate::component) struct Pins;
    pub(in crate::component) fn register_pins;
    pins {
        pendant_snapshot_generation: u32 in => "dmc2-pendant-control.snapshot-generation";
        connected: bit in => "dmc2-pendant-control.connected";
        serial_fault: bit in => "dmc2-pendant-control.serial-fault";
        quadrature_fault: bit in => "dmc2-pendant-control.quadrature-fault";
        pendant_fault_reset_ack: u32 in => "dmc2-pendant-control.pendant-reset-ack";
        pendant_fault_reset_request: u32 out => "dmc2-pendant-control.pendant-reset-request";
        estop_pressed: bit in => "dmc2-pendant-control.estop-pressed";
        deadman_held: bit in => "dmc2-pendant-control.deadman-held";
        selector_valid: bit in => "dmc2-pendant-control.selector-valid";
        axis_code: s32 in => "dmc2-pendant-control.axis-code";
        multiplier_code: s32 in => "dmc2-pendant-control.multiplier-code";
        latest_detent: s32 in => "dmc2-pendant-control.latest-detent";
        detent_count: s32 in => "dmc2-pendant-control.detent-count";
        transition_count: s32 in => "dmc2-pendant-control.transition-count";
        quadrature_errors: u32 in => "dmc2-pendant-control.quadrature-errors";
        sequence: u32 in => "dmc2-pendant-control.sequence";
        milliseconds: u32 in => "dmc2-pendant-control.milliseconds";

        task_snapshot_generation: u32 in => "dmc2-pendant-control.task-snapshot-generation";
        task_monitor_connected: bit in => "dmc2-pendant-control.task-monitor-connected";
        task_monitor_fault: bit in => "dmc2-pendant-control.task-monitor-fault";
        task_heartbeat: u32 in => "dmc2-pendant-control.task-heartbeat";
        machine_on: bit in => "dmc2-pendant-control.machine-on";
        estopped: bit in => "dmc2-pendant-control.estopped";
        manual_mode: bit in => "dmc2-pendant-control.manual-mode";
        joint_mode: bit in => "dmc2-pendant-control.joint-mode";
        teleop_mode: bit in => "dmc2-pendant-control.teleop-mode";
        interp_idle: bit in => "dmc2-pendant-control.interp-idle";
        joint_homed: bit[3] in => [
            "dmc2-pendant-control.joint-0-homed",
            "dmc2-pendant-control.joint-1-homed",
            "dmc2-pendant-control.joint-2-homed",
        ];
        joint_homing: bit[3] in => [
            "dmc2-pendant-control.joint-0-homing",
            "dmc2-pendant-control.joint-1-homing",
            "dmc2-pendant-control.joint-2-homing",
        ];
        axis_stopped: bit[3] in => [
            "dmc2-pendant-control.axis-0-stopped",
            "dmc2-pendant-control.axis-1-stopped",
            "dmc2-pendant-control.axis-2-stopped",
        ];

        motion_enabled: bit in => "dmc2-pendant-control.motion-enabled";
        motion_teleop_mode: bit in => "dmc2-pendant-control.motion-teleop-mode";
        motion_coord_mode: bit in => "dmc2-pendant-control.motion-coord-mode";
        motion_in_position: bit in => "dmc2-pendant-control.motion-in-position";
        motion_jog_active: bit in => "dmc2-pendant-control.motion-jog-active";
        axis_wheel_jog_active: bit[3] in => [
            "dmc2-pendant-control.axis-0-wheel-jog-active",
            "dmc2-pendant-control.axis-1-wheel-jog-active",
            "dmc2-pendant-control.axis-2-wheel-jog-active",
        ];
        joint_wheel_jog_active: bit[3] in => [
            "dmc2-pendant-control.joint-0-wheel-jog-active",
            "dmc2-pendant-control.joint-1-wheel-jog-active",
            "dmc2-pendant-control.joint-2-wheel-jog-active",
        ];
        joint_in_position: bit[3] in => [
            "dmc2-pendant-control.joint-0-in-position",
            "dmc2-pendant-control.joint-1-in-position",
            "dmc2-pendant-control.joint-2-in-position",
        ];
        motor_count: s32[3] in => [
            "dmc2-pendant-control.motor-0-count",
            "dmc2-pendant-control.motor-1-count",
            "dmc2-pendant-control.motor-2-count",
        ];
        motor_position_feedback: float[3] in => [
            "dmc2-pendant-control.motor-0-position-feedback",
            "dmc2-pendant-control.motor-1-position-feedback",
            "dmc2-pendant-control.motor-2-position-feedback",
        ];
        motor_limit_raw: bit[3] in => [
            "dmc2-pendant-control.motor-0-limit-raw",
            "dmc2-pendant-control.motor-1-limit-raw",
            "dmc2-pendant-control.motor-2-limit-raw",
        ];
        motor_limit_latched: bit[3] in => [
            "dmc2-pendant-control.motor-0-limit-latched",
            "dmc2-pendant-control.motor-1-limit-latched",
            "dmc2-pendant-control.motor-2-limit-latched",
        ];
        pendant_mode_enabled: bit in => "dmc2-pendant-control.pendant-mode-enabled";
        servo_thread_ready: bit in => "dmc2-pendant-control.servo-thread-ready";
        mesa_watchdog_has_bit: bit io => "dmc2-pendant-control.mesa-watchdog-has-bit";
        mesa_packet_error: bit in => "dmc2-pendant-control.mesa-packet-error";
        mesa_packet_error_total: u32 in => "dmc2-pendant-control.mesa-packet-error-total";
        mesa_packet_error_exceeded: bit in => "dmc2-pendant-control.mesa-packet-error-exceeded";
        software_watchdog_ok: bit in => "dmc2-pendant-control.software-watchdog-ok";
        ui_ready: bit in => "dmc2-pendant-control.ui-ready";
        linuxcnc_estop_reset_request: bit in => "dmc2-pendant-control.base-estop-reset";

        motor_limit_reset: bit[3] out => [
            "dmc2-pendant-control.motor-0-limit-reset",
            "dmc2-pendant-control.motor-1-limit-reset",
            "dmc2-pendant-control.motor-2-limit-reset",
        ];
        motor_command_enable: bit[3] out => [
            "dmc2-pendant-control.motor-0-command-enable",
            "dmc2-pendant-control.motor-1-command-enable",
            "dmc2-pendant-control.motor-2-command-enable",
        ];
        motor_toward_limit: bit[3] out => [
            "dmc2-pendant-control.motor-0-toward-limit",
            "dmc2-pendant-control.motor-1-toward-limit",
            "dmc2-pendant-control.motor-2-toward-limit",
        ];
        external_enable: bit out => "dmc2-pendant-control.external-enable";
        watchdog_enable: bit out => "dmc2-pendant-control.watchdog-enable";
        heartbeat: bit out => "dmc2-pendant-control.heartbeat";
        position_known: bit out => "dmc2-pendant-control.position-known";
        position_unknown: bit out => "dmc2-pendant-control.position-unknown";
        control_ready: bit out => "dmc2-pendant-control.control-ready";
        fault_reset_allowed: bit out => "dmc2-pendant-control.fault-reset-allowed";
        fault: bit out => "dmc2-pendant-control.fault";
        fault_snapshot_generation: u32 out => "dmc2-pendant-control.fault-snapshot-generation";
        fault_code: s32 out => "dmc2-pendant-control.fault-code";

        fault_evidence_valid: bit out => "dmc2-pendant-control.fault-data-b00";
        fault_axis_valid: bit out => "dmc2-pendant-control.fault-data-b01";
        fault_axis: bit[3] out => [
            "dmc2-pendant-control.fault-data-b02",
            "dmc2-pendant-control.fault-data-b03",
            "dmc2-pendant-control.fault-data-b04",
        ];
        fault_motor_valid: bit out => "dmc2-pendant-control.fault-data-b05";
        fault_start_count_valid: bit out => "dmc2-pendant-control.fault-data-b06";
        fault_target_count_valid: bit out => "dmc2-pendant-control.fault-data-b07";
        fault_observed_count_valid: bit out => "dmc2-pendant-control.fault-data-b08";
        fault_target_position_valid: bit out => "dmc2-pendant-control.fault-data-b09";
        fault_observed_position_valid: bit out => "dmc2-pendant-control.fault-data-b10";
        fault_position_error_valid: bit out => "dmc2-pendant-control.fault-data-b11";
        fault_expected_limit_mask_valid: bit out => "dmc2-pendant-control.fault-data-b12";
        fault_elapsed_valid: bit out => "dmc2-pendant-control.fault-data-b13";
        fault_timeout_valid: bit out => "dmc2-pendant-control.fault-data-b14";
        fault_link_connected: bit out => "dmc2-pendant-control.fault-data-b15";
        fault_serial_fault: bit out => "dmc2-pendant-control.fault-data-b16";
        fault_quadrature_fault: bit out => "dmc2-pendant-control.fault-data-b17";
        fault_pendant_estop_pressed: bit out => "dmc2-pendant-control.fault-data-b18";
        fault_machine_on: bit out => "dmc2-pendant-control.fault-data-b19";
        fault_machine_estopped: bit out => "dmc2-pendant-control.fault-data-b20";
        fault_manual_mode: bit out => "dmc2-pendant-control.fault-data-b21";
        fault_joint_mode: bit out => "dmc2-pendant-control.fault-data-b22";
        fault_teleop_mode: bit out => "dmc2-pendant-control.fault-data-b23";
        fault_interp_idle: bit out => "dmc2-pendant-control.fault-data-b24";
        fault_motion_command_ready: bit out => "dmc2-pendant-control.fault-data-b25";
        fault_task_heartbeat_age_valid: bit out => "dmc2-pendant-control.fault-data-b26";
        fault_pendant_packet_age_valid: bit out => "dmc2-pendant-control.fault-data-b27";
        fault_mesa_phase_valid: bit out => "dmc2-pendant-control.fault-data-b28";
        fault_controller_watchdog_phase_valid: bit out => "dmc2-pendant-control.fault-data-b29";
        fault_motion_enabled: bit out => "dmc2-pendant-control.fault-data-b30";
        fault_motion_teleop_mode: bit out => "dmc2-pendant-control.fault-data-b31";
        fault_motion_coord_mode: bit out => "dmc2-pendant-control.fault-data-b32";
        fault_motion_in_position: bit out => "dmc2-pendant-control.fault-data-b33";
        fault_motion_jog_active: bit out => "dmc2-pendant-control.fault-data-b34";
        fault_consumer_active_seen: bit out => "dmc2-pendant-control.fault-data-b35";
        fault_feedback_progress_seen: bit out => "dmc2-pendant-control.fault-data-b36";
        fault_motor: s32 out => "dmc2-pendant-control.fault-data-s00";
        fault_start_count: s32 out => "dmc2-pendant-control.fault-data-s01";
        fault_target_count: s32 out => "dmc2-pendant-control.fault-data-s02";
        fault_observed_count: s32 out => "dmc2-pendant-control.fault-data-s03";
        fault_supervisor_phase: s32 out => "dmc2-pendant-control.fault-data-s04";
        fault_mesa_phase: s32 out => "dmc2-pendant-control.fault-data-s05";
        fault_controller_watchdog_phase: s32 out => "dmc2-pendant-control.fault-data-s06";
        fault_counts_by_motor: s32[3] out => [
            "dmc2-pendant-control.fault-data-s07",
            "dmc2-pendant-control.fault-data-s08",
            "dmc2-pendant-control.fault-data-s09",
        ];
        fault_target_position_pulses: float out => "dmc2-pendant-control.fault-data-f00";
        fault_observed_position_pulses: float out => "dmc2-pendant-control.fault-data-f01";
        fault_position_error_pulses: float out => "dmc2-pendant-control.fault-data-f02";
        fault_elapsed_seconds: float out => "dmc2-pendant-control.fault-data-f03";
        fault_timeout_seconds: float out => "dmc2-pendant-control.fault-data-f04";
        fault_task_heartbeat_age_seconds: float out => "dmc2-pendant-control.fault-data-f05";
        fault_pendant_packet_age_seconds: float out => "dmc2-pendant-control.fault-data-f06";
        fault_position_feedback_by_motor: float[3] out => [
            "dmc2-pendant-control.fault-data-f07",
            "dmc2-pendant-control.fault-data-f08",
            "dmc2-pendant-control.fault-data-f09",
        ];
        fault_raw_limit_mask: u32 out => "dmc2-pendant-control.fault-data-u00";
        fault_safety_limit_mask: u32 out => "dmc2-pendant-control.fault-data-u01";
        fault_expected_limit_mask: u32 out => "dmc2-pendant-control.fault-data-u02";
        fault_homed_mask: u32 out => "dmc2-pendant-control.fault-data-u03";
        fault_homing_mask: u32 out => "dmc2-pendant-control.fault-data-u04";
        fault_stopped_mask: u32 out => "dmc2-pendant-control.fault-data-u05";
        fault_axis_wheel_jog_active_mask: u32 out => "dmc2-pendant-control.fault-data-u06";
        fault_joint_wheel_jog_active_mask: u32 out => "dmc2-pendant-control.fault-data-u07";
        fault_joint_in_position_mask: u32 out => "dmc2-pendant-control.fault-data-u08";

        recovery_active: bit out => "dmc2-pendant-control.recovery-active";
        jog_active: bit out => "dmc2-pendant-control.jog-active";
        bounce_active: bit out => "dmc2-pendant-control.bounce-active";
        supervisor_phase: s32 out => "dmc2-pendant-control.supervisor-phase";
        mesa_phase: s32 out => "dmc2-pendant-control.mesa-phase";
        controller_watchdog_phase: s32 out => "dmc2-pendant-control.controller-watchdog-phase";
        estop_reset_request: bit out => "dmc2-pendant-control.estop-reset-request";
        machine_on_request: bit out => "dmc2-pendant-control.machine-on-request";

        axis_jog_counts: s32[3] out => [
            "dmc2-pendant-control.axis-0-jog-counts",
            "dmc2-pendant-control.axis-1-jog-counts",
            "dmc2-pendant-control.axis-2-jog-counts",
        ];
        joint_jog_counts: s32[3] out => [
            "dmc2-pendant-control.joint-0-jog-counts",
            "dmc2-pendant-control.joint-1-jog-counts",
            "dmc2-pendant-control.joint-2-jog-counts",
        ];
        axis_jog_scale: float[3] out => [
            "dmc2-pendant-control.axis-0-jog-scale",
            "dmc2-pendant-control.axis-1-jog-scale",
            "dmc2-pendant-control.axis-2-jog-scale",
        ];
        joint_jog_scale: float[3] out => [
            "dmc2-pendant-control.joint-0-jog-scale",
            "dmc2-pendant-control.joint-1-jog-scale",
            "dmc2-pendant-control.joint-2-jog-scale",
        ];
        axis_jog_enable: bit[3] out => [
            "dmc2-pendant-control.axis-0-jog-enable",
            "dmc2-pendant-control.axis-1-jog-enable",
            "dmc2-pendant-control.axis-2-jog-enable",
        ];
        joint_jog_enable: bit[3] out => [
            "dmc2-pendant-control.joint-0-jog-enable",
            "dmc2-pendant-control.joint-1-jog-enable",
            "dmc2-pendant-control.joint-2-jog-enable",
        ];
        axis_jog_vel_mode: bit[3] out => [
            "dmc2-pendant-control.axis-0-jog-vel-mode",
            "dmc2-pendant-control.axis-1-jog-vel-mode",
            "dmc2-pendant-control.axis-2-jog-vel-mode",
        ];
        joint_jog_vel_mode: bit[3] out => [
            "dmc2-pendant-control.joint-0-jog-vel-mode",
            "dmc2-pendant-control.joint-1-jog-vel-mode",
            "dmc2-pendant-control.joint-2-jog-vel-mode",
        ];
        jog_stop: bit out => "dmc2-pendant-control.jog-stop";
        jog_stop_immediate: bit out => "dmc2-pendant-control.jog-stop-immediate";
        command_phase: s32 out => "dmc2-pendant-control.command-phase";
    }
    numbered {
        fault_kind: bit[FaultCode::COUNT] out => (
            "dmc2-pendant-control.fault-kind-",
            FaultCode::ALL
        );
        fault_supervisor_phase_kind: bit[Phase::COUNT] out => (
            "dmc2-pendant-control.fault-data-sup-kind-",
            Phase::ALL
        );
        fault_mesa_phase_kind: bit[MesaStartupPhase::COUNT] out => (
            "dmc2-pendant-control.fault-data-mesa-kind-",
            MesaStartupPhase::ALL
        );
        fault_controller_watchdog_phase_kind: bit[ControllerWatchdogPhase::COUNT] out => (
            "dmc2-pendant-control.fault-data-wd-kind-",
            ControllerWatchdogPhase::ALL
        );
        supervisor_phase_kind: bit[Phase::COUNT] out => (
            "dmc2-pendant-control.supervisor-phase-kind-",
            Phase::ALL
        );
        mesa_phase_kind: bit[MesaStartupPhase::COUNT] out => (
            "dmc2-pendant-control.mesa-phase-kind-",
            MesaStartupPhase::ALL
        );
        controller_watchdog_phase_kind: bit[ControllerWatchdogPhase::COUNT] out => (
            "dmc2-pendant-control.watchdog-phase-kind-",
            ControllerWatchdogPhase::ALL
        );
        command_phase_kind: bit[MotionCommandPhase::COUNT] out => (
            "dmc2-pendant-control.command-phase-kind-",
            MotionCommandPhase::ALL
        );
    }
}
