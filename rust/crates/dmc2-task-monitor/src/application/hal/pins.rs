use dmc2_hal_sys as hal;
use dmc2_linuxcnc_interface::{CMS_STATUS, NML_ERROR};

pub(super) const NML_ERROR_KIND_COUNT: usize = NML_ERROR.codes.len();
pub(super) const CMS_STATUS_KIND_COUNT: usize = CMS_STATUS.codes.len();

hal::userspace_hal_pin_catalog! {
    pub(super) struct ControllerFaultEvidencePins;
    error = super::registration::RegistrationError;
    register = super::registration::new_pin;
    pins {
    valid: bit in => "ctl-fault-data-b00-in";
    axis_valid: bit in => "ctl-fault-data-b01-in";
    axis: bit[3] in => ["ctl-fault-data-b02-in", "ctl-fault-data-b03-in", "ctl-fault-data-b04-in"];
    motor_valid: bit in => "ctl-fault-data-b05-in";
    start_count_valid: bit in => "ctl-fault-data-b06-in";
    target_count_valid: bit in => "ctl-fault-data-b07-in";
    observed_count_valid: bit in => "ctl-fault-data-b08-in";
    target_position_valid: bit in => "ctl-fault-data-b09-in";
    observed_position_valid: bit in => "ctl-fault-data-b10-in";
    position_error_valid: bit in => "ctl-fault-data-b11-in";
    expected_limit_mask_valid: bit in => "ctl-fault-data-b12-in";
    elapsed_valid: bit in => "ctl-fault-data-b13-in";
    timeout_valid: bit in => "ctl-fault-data-b14-in";
    link_connected: bit in => "ctl-fault-data-b15-in";
    serial_fault: bit in => "ctl-fault-data-b16-in";
    quadrature_fault: bit in => "ctl-fault-data-b17-in";
    pendant_estop_pressed: bit in => "ctl-fault-data-b18-in";
    machine_on: bit in => "ctl-fault-data-b19-in";
    machine_estopped: bit in => "ctl-fault-data-b20-in";
    manual_mode: bit in => "ctl-fault-data-b21-in";
    joint_mode: bit in => "ctl-fault-data-b22-in";
    teleop_mode: bit in => "ctl-fault-data-b23-in";
    interp_idle: bit in => "ctl-fault-data-b24-in";
    motion_command_ready: bit in => "ctl-fault-data-b25-in";
    task_heartbeat_age_valid: bit in => "ctl-fault-data-b26-in";
    pendant_packet_age_valid: bit in => "ctl-fault-data-b27-in";
    mesa_phase_valid: bit in => "ctl-fault-data-b28-in";
    controller_watchdog_phase_valid: bit in => "ctl-fault-data-b29-in";
    motion_enabled: bit in => "ctl-fault-data-b30-in";
    motion_teleop_mode: bit in => "ctl-fault-data-b31-in";
    motion_coord_mode: bit in => "ctl-fault-data-b32-in";
    motion_in_position: bit in => "ctl-fault-data-b33-in";
    motion_jog_active: bit in => "ctl-fault-data-b34-in";
    consumer_active_seen: bit in => "ctl-fault-data-b35-in";
    feedback_progress_seen: bit in => "ctl-fault-data-b36-in";
    motor: s32 in => "ctl-fault-data-s00-in";
    start_count: s32 in => "ctl-fault-data-s01-in";
    target_count: s32 in => "ctl-fault-data-s02-in";
    observed_count: s32 in => "ctl-fault-data-s03-in";
    counts_by_motor: s32[3] in => ["ctl-fault-data-s07-in", "ctl-fault-data-s08-in", "ctl-fault-data-s09-in"];
    supervisor_phase: s32 in => "ctl-fault-data-s04-in";
    mesa_phase: s32 in => "ctl-fault-data-s05-in";
    controller_watchdog_phase: s32 in => "ctl-fault-data-s06-in";
    raw_limit_mask: u32 in => "ctl-fault-data-u00-in";
    safety_limit_mask: u32 in => "ctl-fault-data-u01-in";
    expected_limit_mask: u32 in => "ctl-fault-data-u02-in";
    homed_mask: u32 in => "ctl-fault-data-u03-in";
    homing_mask: u32 in => "ctl-fault-data-u04-in";
    stopped_mask: u32 in => "ctl-fault-data-u05-in";
    axis_wheel_jog_active_mask: u32 in => "ctl-fault-data-u06-in";
    joint_wheel_jog_active_mask: u32 in => "ctl-fault-data-u07-in";
    joint_in_position_mask: u32 in => "ctl-fault-data-u08-in";
    target_position_pulses: float in => "ctl-fault-data-f00-in";
    observed_position_pulses: float in => "ctl-fault-data-f01-in";
    position_error_pulses: float in => "ctl-fault-data-f02-in";
    position_feedback_by_motor: float[3] in => ["ctl-fault-data-f07-in", "ctl-fault-data-f08-in", "ctl-fault-data-f09-in"];
    elapsed_seconds: float in => "ctl-fault-data-f03-in";
    timeout_seconds: float in => "ctl-fault-data-f04-in";
    task_heartbeat_age_seconds: float in => "ctl-fault-data-f05-in";
    pendant_packet_age_seconds: float in => "ctl-fault-data-f06-in";
    }
    groups {}
}

hal::userspace_hal_pin_catalog! {
    pub(super) struct SerialBridgeFaultEvidencePins;
    error = super::registration::RegistrationError;
    register = super::registration::new_pin;
    pins {
    protocol_error_valid: bit in => "ser-fault-data-b00-in";
    line_bytes_valid: bit in => "ser-fault-data-b01-in";
    previous_sequence_valid: bit in => "ser-fault-data-b02-in";
    observed_sequence_valid: bit in => "ser-fault-data-b03-in";
    packet_age_valid: bit in => "ser-fault-data-b04-in";
    timeout_valid: bit in => "ser-fault-data-b05-in";
    previous_quadrature_errors_valid: bit in => "ser-fault-data-b06-in";
    observed_quadrature_errors_valid: bit in => "ser-fault-data-b07-in";
    operating_system_error_valid: bit in => "ser-fault-data-b08-in";
    transport_contract_result_valid: bit in => "ser-fault-data-b09-in";
    protocol_error_code: s32 in => "ser-fault-data-s00-in";
    operating_system_error: s32 in => "ser-fault-data-s01-in";
    line_bytes: u32 in => "ser-fault-data-u00-in";
    previous_sequence: u32 in => "ser-fault-data-u01-in";
    observed_sequence: u32 in => "ser-fault-data-u02-in";
    previous_quadrature_errors: u32 in => "ser-fault-data-u03-in";
    observed_quadrature_errors: u32 in => "ser-fault-data-u04-in";
    transport_contract_result_low: u32 in => "ser-fault-data-u05-in";
    transport_contract_result_high: u32 in => "ser-fault-data-u06-in";
    packet_age_ms: float in => "ser-fault-data-f00-in";
    timeout_ms: float in => "ser-fault-data-f01-in";
    }
    groups {}
}

hal::userspace_hal_pin_catalog! {
    pub(super) struct H100FaultEvidencePins;
    error = super::registration::RegistrationError;
    register = super::registration::new_pin;
    pins {
    valid: bit in => "h100-fault-data-b00-in";
    context_fault_latched_before: bit in => "h100-fault-data-b01-in";
    context_fault_record_present_before: bit in => "h100-fault-data-b02-in";
    machine_enabled: bit in => "h100-fault-data-b03-in";
    run_request: bit in => "h100-fault-data-b04-in";
    forward_request: bit in => "h100-fault-data-b05-in";
    reverse_request: bit in => "h100-fault-data-b06-in";
    reset: bit in => "h100-fault-data-b07-in";
    link_fault: bit in => "h100-fault-data-b08-in";
    command_disabled: bit in => "h100-fault-data-b09-in";
    state_before: s32 in => "h100-fault-data-s00-in";
    context_fault_code_before: u32 in => "h100-fault-data-u00-in";
    context_fault_record_code_before: u32 in => "h100-fault-data-u01-in";
    control_mode_f001: u32 in => "h100-fault-data-u02-in";
    frequency_source_f002: u32 in => "h100-fault-data-u03-in";
    reference_f004_centihz: u32 in => "h100-fault-data-u04-in";
    maximum_f005_centihz: u32 in => "h100-fault-data-u05-in";
    lower_limit_f011_centihz: u32 in => "h100-fault-data-u06-in";
    panel_stop_f024: u32 in => "h100-fault-data-u07-in";
    slave_address_f163: u32 in => "h100-fault-data-u08-in";
    baud_selector_f164: u32 in => "h100-fault-data-u09-in";
    data_mode_f165: u32 in => "h100-fault-data-u10-in";
    frequency_decimals_f169: u32 in => "h100-fault-data-u11-in";
    output_frequency_decihz: u32 in => "h100-fault-data-u12-in";
    current_vfd_fault: u32 in => "h100-fault-data-u13-in";
    main_status: u32 in => "h100-fault-data-u14-in";
    given_frequency_readback: u32 in => "h100-fault-data-u15-in";
    expected_reference_f004_centihz: u32 in => "h100-fault-data-u16-in";
    expected_maximum_f005_centihz: u32 in => "h100-fault-data-u17-in";
    calculated_frequency_register: u32 in => "h100-fault-data-u18-in";
    speed_command_rpm: float in => "h100-fault-data-f00-in";
    rated_rpm: float in => "h100-fault-data-f01-in";
    minimum_rpm: float in => "h100-fault-data-f02-in";
    maximum_rpm: float in => "h100-fault-data-f03-in";
    at_speed_tolerance_hz: float in => "h100-fault-data-f04-in";
    calculated_frequency_hz: float in => "h100-fault-data-f05-in";
    }
    groups {}
}

hal::userspace_hal_pin_catalog! {
    pub(super) struct HalPins;
    error = super::registration::RegistrationError;
    register = super::registration::new_pin;
    pins {
    snapshot_generation: u32 out => "snapshot-generation";
    connected: bit out => "connected";
    fault: bit out => "fault";
    task_heartbeat: u32 out => "task-heartbeat";
    publications: u32 out => "publications";
    poll_errors: u32 out => "poll-errors";
    nml_error_code: s32 out => "nml-error-code";
    nml_error_known: bit out => "nml-error-known";
    nml_error_unknown: bit out => "nml-error-unknown";
    nml_error_kind: bit[NML_ERROR_KIND_COUNT] out => std::array::from_fn::<String, NML_ERROR_KIND_COUNT, _>(|index| format!("nml-kind-{index}"));
    cms_status_code: s32 out => "cms-status-code";
    cms_status_known: bit out => "cms-status-known";
    cms_status_unknown: bit out => "cms-status-unknown";
    cms_status_kind: bit[CMS_STATUS_KIND_COUNT] out => std::array::from_fn::<String, CMS_STATUS_KIND_COUNT, _>(|index| format!("cms-kind-{index}"));
    linuxcnc_error_active: bit out => "linuxcnc-error-active";
    linuxcnc_warning_active: bit out => "linuxcnc-warning-active";
    unknown_code_active: bit out => "unknown-code-active";
    active_error_mask_low: u32 out => "active-error-mask-low";
    active_error_mask_high: u32 out => "active-error-mask-high";
    active_warning_mask_low: u32 out => "active-warning-mask-low";
    active_warning_mask_high: u32 out => "active-warning-mask-high";
    latched_error_mask_low: u32 out => "latched-error-mask-low";
    latched_error_mask_high: u32 out => "latched-error-mask-high";
    latched_warning_mask_low: u32 out => "latched-warning-mask-low";
    latched_warning_mask_high: u32 out => "latched-warning-mask-high";
    unknown_domain_mask_low: u32 out => "unknown-domain-mask-low";
    unknown_domain_mask_high: u32 out => "unknown-domain-mask-high";
    diagnostic_count: u32 out => "diagnostic-count";
    unknown_code_count: u32 out => "unknown-code-count";
    diagnostic_transitions: u32 out => "diagnostic-transitions";
    latest_code_domain: s32 out => "latest-code-domain";
    latest_code_low: u32 out => "latest-code-low";
    latest_code_high: u32 out => "latest-code-high";
    latest_journal_sequence_low: u32 out => "latest-journal-sequence-low";
    latest_journal_sequence_high: u32 out => "latest-journal-sequence-high";
    latest_severity: s32 out => "latest-severity";
    latest_action: s32 out => "latest-action";
    latest_code_known: bit out => "latest-code-known";
    latest_code_unknown: bit out => "latest-code-unknown";
    clear_latched: bit in => "clear-latched";
    serial_bridge_fault_valid_in: bit in => "serial-bridge-fault-valid-in";
    spindle_fault_latched_in: bit in => "spindle-fault-latched-in";
    controller_fault_snapshot_generation_in: u32 in => "ctl-fd-generation-in";
    controller_fault_code_in: s32 in => "controller-fault-code-in";
    serial_bridge_snapshot_generation_in: u32 in => "ser-fd-generation-in";
    serial_bridge_fault_code_in: s32 in => "serial-bridge-fault-code-in";
    h100_diagnostic_snapshot_generation_in: u32 in => "h100-fd-generation-in";
    spindle_fault_code_in: u32 in => "spindle-fault-code-in";
    spindle_block_code_in: u32 in => "spindle-block-code-in";
    h100_vfd_fault_code_in: u32 in => "h100-vfd-fault-code-in";
    h100_main_status_in: u32 in => "h100-main-status-in";
    snapshot_abi_version: u32 out => "snapshot-abi-version";
    snapshot_struct_size: u32 out => "snapshot-struct-size";
    machine_on: bit out => "machine-on";
    estopped: bit out => "estopped";
    manual_mode: bit out => "manual-mode";
    joint_mode: bit out => "joint-mode";
    teleop_mode: bit out => "teleop-mode";
    interp_idle: bit out => "interp-idle";
    homed: bit[3] out => std::array::from_fn::<String, 3, _>(|index| format!("joint-{index}-homed"));
    homing: bit[3] out => std::array::from_fn::<String, 3, _>(|index| format!("joint-{index}-homing"));
    axis_stopped: bit[3] out => std::array::from_fn::<String, 3, _>(|index| format!("axis-{index}-stopped"));
    }
    groups {
        controller_fault_evidence: ControllerFaultEvidencePins;
        serial_bridge_fault_evidence: SerialBridgeFaultEvidencePins;
        h100_fault_evidence: H100FaultEvidencePins;
    }
}
