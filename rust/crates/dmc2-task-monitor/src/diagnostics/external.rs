//! Diagnostics imported from the other compiled DMC2 HAL components.

use dmc2_core::startup::{ControllerWatchdogPhase, MesaStartupPhase};
use dmc2_core::supervisor::{FaultCode as ControllerFaultCode, Phase as ControllerPhase};
use dmc2_diagnostics::{
    DiagnosticMetadata, RecoverableDiagnostic, RecoveryClass, RecoveryClassified,
    SelfDescribingDiagnostic, UnknownDiagnostic,
};
use dmc2_serial_bridge::{BridgeFaultCode, ProtocolError};
use h100_spindle::sequencer::{
    main_status_reserved_mask, BlockCode as SpindleCode, MainStatusBit, State as SpindleState,
    VfdFaultCode,
};

use super::category::{self, DiagnosticCategory};
use super::report::{DiagnosticReport, Issue, Severity};

const CONTROLLER_DOMAIN_ID: u32 = 1_000;
const SERIAL_BRIDGE_DOMAIN_ID: u32 = 1_001;
const SPINDLE_FAULT_DOMAIN_ID: u32 = 1_002;
const SPINDLE_BLOCK_DOMAIN_ID: u32 = 1_003;
const H100_VFD_DOMAIN_ID: u32 = 1_004;
const H100_MAIN_STATUS_DOMAIN_ID: u32 = 1_005;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControllerFaultEvidenceSnapshot {
    pub snapshot_generation: u32,
    pub valid: bool,
    pub axis_valid: bool,
    pub axis: [bool; 3],
    pub motor_valid: bool,
    pub motor: i32,
    pub start_count_valid: bool,
    pub start_count: i32,
    pub target_count_valid: bool,
    pub target_count: i32,
    pub observed_count_valid: bool,
    pub observed_count: i32,
    pub target_position_valid: bool,
    pub target_position_pulses: f64,
    pub observed_position_valid: bool,
    pub observed_position_pulses: f64,
    pub position_error_valid: bool,
    pub position_error_pulses: f64,
    pub counts_by_motor: [i32; 3],
    pub position_feedback_by_motor: [f64; 3],
    pub raw_limit_mask: u32,
    pub safety_limit_mask: u32,
    pub expected_limit_mask_valid: bool,
    pub expected_limit_mask: u32,
    pub elapsed_valid: bool,
    pub elapsed_seconds: f64,
    pub timeout_valid: bool,
    pub timeout_seconds: f64,
    pub link_connected: bool,
    pub serial_fault: bool,
    pub quadrature_fault: bool,
    pub pendant_estop_pressed: bool,
    pub machine_on: bool,
    pub machine_estopped: bool,
    pub manual_mode: bool,
    pub joint_mode: bool,
    pub teleop_mode: bool,
    pub interp_idle: bool,
    pub homed_mask: u32,
    pub homing_mask: u32,
    pub stopped_mask: u32,
    pub motion_command_ready: bool,
    pub motion_enabled: bool,
    pub motion_teleop_mode: bool,
    pub motion_coord_mode: bool,
    pub motion_in_position: bool,
    pub motion_jog_active: bool,
    pub consumer_active_seen: bool,
    pub feedback_progress_seen: bool,
    pub axis_wheel_jog_active_mask: u32,
    pub joint_wheel_jog_active_mask: u32,
    pub joint_in_position_mask: u32,
    pub task_heartbeat_age_valid: bool,
    pub task_heartbeat_age_seconds: f64,
    pub pendant_packet_age_valid: bool,
    pub pendant_packet_age_seconds: f64,
    pub supervisor_phase: i32,
    pub mesa_phase_valid: bool,
    pub mesa_phase: i32,
    pub controller_watchdog_phase_valid: bool,
    pub controller_watchdog_phase: i32,
}

impl Default for ControllerFaultEvidenceSnapshot {
    fn default() -> Self {
        Self {
            snapshot_generation: 0,
            valid: false,
            axis_valid: false,
            axis: [false; 3],
            motor_valid: false,
            motor: 0,
            start_count_valid: false,
            start_count: 0,
            target_count_valid: false,
            target_count: 0,
            observed_count_valid: false,
            observed_count: 0,
            target_position_valid: false,
            target_position_pulses: 0.0,
            observed_position_valid: false,
            observed_position_pulses: 0.0,
            position_error_valid: false,
            position_error_pulses: 0.0,
            counts_by_motor: [0; 3],
            position_feedback_by_motor: [0.0; 3],
            raw_limit_mask: 0,
            safety_limit_mask: 0,
            expected_limit_mask_valid: false,
            expected_limit_mask: 0,
            elapsed_valid: false,
            elapsed_seconds: 0.0,
            timeout_valid: false,
            timeout_seconds: 0.0,
            link_connected: false,
            serial_fault: false,
            quadrature_fault: false,
            pendant_estop_pressed: false,
            machine_on: false,
            machine_estopped: false,
            manual_mode: false,
            joint_mode: false,
            teleop_mode: false,
            interp_idle: false,
            homed_mask: 0,
            homing_mask: 0,
            stopped_mask: 0,
            motion_command_ready: false,
            motion_enabled: false,
            motion_teleop_mode: false,
            motion_coord_mode: false,
            motion_in_position: false,
            motion_jog_active: false,
            consumer_active_seen: false,
            feedback_progress_seen: false,
            axis_wheel_jog_active_mask: 0,
            joint_wheel_jog_active_mask: 0,
            joint_in_position_mask: 0,
            task_heartbeat_age_valid: false,
            task_heartbeat_age_seconds: 0.0,
            pendant_packet_age_valid: false,
            pendant_packet_age_seconds: 0.0,
            supervisor_phase: 0,
            mesa_phase_valid: false,
            mesa_phase: 0,
            controller_watchdog_phase_valid: false,
            controller_watchdog_phase: 0,
        }
    }
}

fn phase_identity<T>(
    value: i32,
    domain: &'static str,
    decode: impl FnOnce(i32) -> Option<T>,
    name: impl FnOnce(T) -> &'static str,
) -> String {
    decode(value).map_or_else(
        || UnknownDiagnostic::new(domain, i64::from(value)).to_string(),
        |phase| format!("{}(raw={value})", name(phase)),
    )
}

impl ControllerFaultEvidenceSnapshot {
    fn render(self) -> String {
        let controller_phase = phase_identity(
            self.supervisor_phase,
            "dmc2_controller_phase",
            ControllerPhase::from_wire_code,
            ControllerPhase::name,
        );
        let mesa_phase = phase_identity(
            self.mesa_phase,
            "dmc2_mesa_startup_phase",
            MesaStartupPhase::from_wire_code,
            MesaStartupPhase::name,
        );
        let watchdog_phase = phase_identity(
            self.controller_watchdog_phase,
            "dmc2_controller_watchdog_phase",
            ControllerWatchdogPhase::from_wire_code,
            ControllerWatchdogPhase::name,
        );
        format!(
            concat!(
                "snapshot_generation={} evidence_valid={} axis_valid={} axis_bits=[{},{},{}] motor_valid={} motor={} ",
                "start_count_valid={} start_count={} target_count_valid={} target_count={} ",
                "observed_count_valid={} observed_count={} target_position_valid={} target_position_pulses={:?} ",
                "observed_position_valid={} observed_position_pulses={:?} position_error_valid={} position_error_pulses={:?} ",
                "counts_by_motor={:?} position_feedback_by_motor={:?} raw_limit_mask=0x{:08x} safety_limit_mask=0x{:08x} ",
                "expected_limit_mask_valid={} expected_limit_mask=0x{:08x} elapsed_valid={} elapsed_seconds={:?} ",
                "timeout_valid={} timeout_seconds={:?} link_connected={} serial_fault={} quadrature_fault={} ",
                "pendant_estop_pressed={} machine_on={} machine_estopped={} manual_mode={} joint_mode={} teleop_mode={} ",
                "interp_idle={} homed_mask=0x{:08x} homing_mask=0x{:08x} stopped_mask=0x{:08x} motion_command_ready={} ",
                "motion_enabled={} motion_teleop_mode={} motion_coord_mode={} motion_in_position={} motion_jog_active={} ",
                "axis_wheel_jog_active_mask=0x{:08x} joint_wheel_jog_active_mask=0x{:08x} joint_in_position_mask=0x{:08x} ",
                "consumer_active_seen={} feedback_progress_seen={} ",
                "task_heartbeat_age_valid={} task_heartbeat_age_seconds={:?} pendant_packet_age_valid={} pendant_packet_age_seconds={:?} ",
                "supervisor_phase={} mesa_phase_valid={} mesa_phase={} controller_watchdog_phase_valid={} controller_watchdog_phase={}"
            ),
            self.snapshot_generation,
            self.valid,
            self.axis_valid,
            self.axis[0], self.axis[1], self.axis[2],
            self.motor_valid, self.motor,
            self.start_count_valid, self.start_count,
            self.target_count_valid, self.target_count,
            self.observed_count_valid, self.observed_count,
            self.target_position_valid, self.target_position_pulses,
            self.observed_position_valid, self.observed_position_pulses,
            self.position_error_valid, self.position_error_pulses,
            self.counts_by_motor, self.position_feedback_by_motor,
            self.raw_limit_mask, self.safety_limit_mask,
            self.expected_limit_mask_valid, self.expected_limit_mask,
            self.elapsed_valid, self.elapsed_seconds,
            self.timeout_valid, self.timeout_seconds,
            self.link_connected, self.serial_fault, self.quadrature_fault,
            self.pendant_estop_pressed, self.machine_on, self.machine_estopped,
            self.manual_mode, self.joint_mode, self.teleop_mode,
            self.interp_idle, self.homed_mask, self.homing_mask, self.stopped_mask,
            self.motion_command_ready,
            self.motion_enabled, self.motion_teleop_mode, self.motion_coord_mode,
            self.motion_in_position, self.motion_jog_active,
            self.axis_wheel_jog_active_mask, self.joint_wheel_jog_active_mask,
            self.joint_in_position_mask,
            self.consumer_active_seen, self.feedback_progress_seen,
            self.task_heartbeat_age_valid, self.task_heartbeat_age_seconds,
            self.pendant_packet_age_valid, self.pendant_packet_age_seconds,
            controller_phase,
            self.mesa_phase_valid, mesa_phase,
            self.controller_watchdog_phase_valid, watchdog_phase,
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SerialBridgeFaultEvidenceSnapshot {
    pub snapshot_generation: u32,
    pub protocol_error_valid: bool,
    pub protocol_error_code: i32,
    pub line_bytes_valid: bool,
    pub line_bytes: u32,
    pub previous_sequence_valid: bool,
    pub previous_sequence: u32,
    pub observed_sequence_valid: bool,
    pub observed_sequence: u32,
    pub packet_age_valid: bool,
    pub packet_age_ms: f64,
    pub timeout_valid: bool,
    pub timeout_ms: f64,
    pub previous_quadrature_errors_valid: bool,
    pub previous_quadrature_errors: u32,
    pub observed_quadrature_errors_valid: bool,
    pub observed_quadrature_errors: u32,
    pub operating_system_error_valid: bool,
    pub operating_system_error: i32,
    pub transport_contract_result_valid: bool,
    pub transport_contract_result: i64,
}

impl SerialBridgeFaultEvidenceSnapshot {
    fn render(self) -> String {
        let protocol = if !self.protocol_error_valid {
            format!("not-present(raw={})", self.protocol_error_code)
        } else if let Some(error) = ProtocolError::from_wire_code(self.protocol_error_code) {
            let metadata = error.metadata();
            format!(
                "{}(raw={} cause={:?} action={:?})",
                metadata.name(),
                self.protocol_error_code,
                metadata.summary(),
                metadata.action()
            )
        } else {
            format!(
                "UNKNOWN_DMC2_SERIAL_PROTOCOL_ERROR(raw={})",
                self.protocol_error_code
            )
        };
        let operating_system_error = if self.operating_system_error_valid {
            format!(
                "errno={}({})",
                self.operating_system_error,
                std::io::Error::from_raw_os_error(self.operating_system_error)
            )
        } else {
            format!("not-present(raw={})", self.operating_system_error)
        };
        format!(
            concat!(
                "snapshot_generation={} protocol_error={} line_bytes_valid={} line_bytes={} ",
                "previous_sequence_valid={} previous_sequence={} observed_sequence_valid={} observed_sequence={} ",
                "packet_age_valid={} packet_age_ms={:?} timeout_valid={} timeout_ms={:?} ",
                "previous_quadrature_errors_valid={} previous_quadrature_errors={} ",
                "observed_quadrature_errors_valid={} observed_quadrature_errors={} ",
                "operating_system_error={} transport_contract_result_valid={} transport_contract_result={}"
            ),
            self.snapshot_generation,
            protocol,
            self.line_bytes_valid,
            self.line_bytes,
            self.previous_sequence_valid,
            self.previous_sequence,
            self.observed_sequence_valid,
            self.observed_sequence,
            self.packet_age_valid,
            self.packet_age_ms,
            self.timeout_valid,
            self.timeout_ms,
            self.previous_quadrature_errors_valid,
            self.previous_quadrature_errors,
            self.observed_quadrature_errors_valid,
            self.observed_quadrature_errors,
            operating_system_error,
            self.transport_contract_result_valid,
            self.transport_contract_result,
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct H100FaultEvidenceSnapshot {
    pub snapshot_generation: u32,
    pub valid: bool,
    pub state_before: i32,
    pub context_fault_latched_before: bool,
    pub context_fault_record_present_before: bool,
    pub context_fault_code_before: u32,
    pub context_fault_record_code_before: u32,
    pub machine_enabled: bool,
    pub run_request: bool,
    pub forward_request: bool,
    pub reverse_request: bool,
    pub reset: bool,
    pub link_fault: bool,
    pub command_disabled: bool,
    pub speed_command_rpm: f64,
    pub control_mode_f001: u32,
    pub frequency_source_f002: u32,
    pub reference_f004_centihz: u32,
    pub maximum_f005_centihz: u32,
    pub lower_limit_f011_centihz: u32,
    pub panel_stop_f024: u32,
    pub slave_address_f163: u32,
    pub baud_selector_f164: u32,
    pub data_mode_f165: u32,
    pub frequency_decimals_f169: u32,
    pub output_frequency_decihz: u32,
    pub current_vfd_fault: u32,
    pub main_status: u32,
    pub given_frequency_readback: u32,
    pub expected_reference_f004_centihz: u32,
    pub expected_maximum_f005_centihz: u32,
    pub calculated_frequency_register: u32,
    pub rated_rpm: f64,
    pub minimum_rpm: f64,
    pub maximum_rpm: f64,
    pub at_speed_tolerance_hz: f64,
    pub calculated_frequency_hz: f64,
}

fn h100_block_identity(raw: u32) -> String {
    SpindleCode::from_wire_code(raw).map_or_else(
        || format!("UNKNOWN_H100_SPINDLE_BLOCK(raw={raw})"),
        |code| format!("{}(raw={raw})", code.metadata().name()),
    )
}

fn h100_vfd_identity(raw: u32) -> String {
    if raw == 0 {
        return "H100_VFD_NO_FAULT(raw=0)".to_owned();
    }
    VfdFaultCode::decode(raw).map_or_else(
        || format!("UNKNOWN_H100_VFD_CURRENT_FAULT(raw={raw})"),
        |code| {
            format!(
                "{}(raw={raw} drive_display={}.{})",
                code.metadata().name(),
                code.family.display(),
                code.phase.display()
            )
        },
    )
}

fn h100_main_status_identity(raw: u32) -> String {
    let names = MainStatusBit::ALL
        .iter()
        .copied()
        .filter(|bit| raw & bit.wire_code() != 0)
        .map(|bit| bit.metadata().name())
        .collect::<Vec<_>>();
    format!(
        "raw=0x{raw:08x} named_bits={names:?} reserved_mask=0x{:08x}",
        main_status_reserved_mask(raw)
    )
}

impl H100FaultEvidenceSnapshot {
    fn render(self) -> String {
        let state = SpindleState::from_raw(self.state_before).map_or_else(
            || format!("UNKNOWN_H100_SPINDLE_STATE(raw={})", self.state_before),
            |state| format!("{}(raw={})", state.metadata().name(), self.state_before),
        );
        format!(
            concat!(
                "snapshot_generation={} evidence_valid={} state_before={} ",
                "context_fault_latched_before={} context_fault_record_present_before={} ",
                "context_fault_code_before={} context_fault_record_code_before={} ",
                "machine_enabled={} run_request={} forward_request={} reverse_request={} reset={} ",
                "link_fault={} command_disabled={} speed_command_rpm={:?} ",
                "control_mode_f001={} frequency_source_f002={} reference_f004_centihz={} ",
                "maximum_f005_centihz={} lower_limit_f011_centihz={} panel_stop_f024={} ",
                "slave_address_f163={} baud_selector_f164={} data_mode_f165={} frequency_decimals_f169={} ",
                "output_frequency_decihz={} current_vfd_fault={} main_status={} given_frequency_readback={} ",
                "expected_reference_f004_centihz={} expected_maximum_f005_centihz={} ",
                "calculated_frequency_register={} rated_rpm={:?} minimum_rpm={:?} maximum_rpm={:?} ",
                "at_speed_tolerance_hz={:?} calculated_frequency_hz={:?}"
            ),
            self.snapshot_generation,
            self.valid,
            state,
            self.context_fault_latched_before,
            self.context_fault_record_present_before,
            h100_block_identity(self.context_fault_code_before),
            h100_block_identity(self.context_fault_record_code_before),
            self.machine_enabled,
            self.run_request,
            self.forward_request,
            self.reverse_request,
            self.reset,
            self.link_fault,
            self.command_disabled,
            self.speed_command_rpm,
            self.control_mode_f001,
            self.frequency_source_f002,
            self.reference_f004_centihz,
            self.maximum_f005_centihz,
            self.lower_limit_f011_centihz,
            self.panel_stop_f024,
            self.slave_address_f163,
            self.baud_selector_f164,
            self.data_mode_f165,
            self.frequency_decimals_f169,
            self.output_frequency_decihz,
            h100_vfd_identity(self.current_vfd_fault),
            h100_main_status_identity(self.main_status),
            self.given_frequency_readback,
            self.expected_reference_f004_centihz,
            self.expected_maximum_f005_centihz,
            self.calculated_frequency_register,
            self.rated_rpm,
            self.minimum_rpm,
            self.maximum_rpm,
            self.at_speed_tolerance_hz,
            self.calculated_frequency_hz,
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ExternalDiagnosticSnapshot {
    pub controller_fault_code: i32,
    pub controller_fault_evidence: ControllerFaultEvidenceSnapshot,
    pub serial_bridge_fault_valid: bool,
    pub serial_bridge_fault_code: i32,
    pub serial_bridge_fault_evidence: SerialBridgeFaultEvidenceSnapshot,
    pub spindle_fault_latched: bool,
    pub spindle_fault_code: u32,
    pub spindle_block_code: u32,
    pub h100_vfd_fault_code: u32,
    pub h100_main_status: u32,
    pub h100_fault_evidence: H100FaultEvidenceSnapshot,
}

#[allow(clippy::too_many_arguments)]
fn push_recoverable_with_evidence<T: RecoverableDiagnostic>(
    report: &mut DiagnosticReport,
    severity: Severity,
    category_value: DiagnosticCategory,
    source: &'static str,
    domain: &'static str,
    domain_id: u32,
    diagnostic: T,
    evidence: String,
) {
    let metadata = diagnostic.metadata();
    report.push(Issue::new(
        severity,
        category_value,
        source,
        domain,
        domain_id,
        metadata.wire_code(),
        Some(metadata.name()),
        metadata.summary(),
        metadata.action(),
        &diagnostic,
        evidence,
    ));
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UnknownExternalDiagnostic {
    CatalogMismatch,
    VfdCurrentFault,
}

impl RecoveryClassified for UnknownExternalDiagnostic {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::CatalogMismatch => RecoveryClass::RelaunchApplication,
            Self::VfdCurrentFault => RecoveryClass::ResetSpindle,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum SpindleDiagnostic {
    Block(SpindleCode),
    VfdFault(VfdFaultCode),
}

impl SelfDescribingDiagnostic for SpindleDiagnostic {
    fn metadata(self) -> DiagnosticMetadata {
        match self {
            Self::Block(code) => code.metadata(),
            Self::VfdFault(code) => code.metadata(),
        }
    }
}

impl RecoveryClassified for SpindleDiagnostic {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::Block(code) => spindle_recovery_class(*code),
            Self::VfdFault(_) => RecoveryClass::ResetSpindle,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_unknown_with_evidence(
    report: &mut DiagnosticReport,
    source: &'static str,
    domain: &'static str,
    domain_id: u32,
    value: i64,
    detail: &'static str,
    operator_action: &'static str,
    recovery_source: UnknownExternalDiagnostic,
    evidence: String,
) {
    report.push(Issue::new(
        Severity::Error,
        category::UNKNOWN_CODE,
        source,
        domain,
        domain_id,
        value,
        None,
        detail,
        operator_action,
        &recovery_source,
        evidence,
    ));
}

fn push_inconsistent(
    report: &mut DiagnosticReport,
    source: &'static str,
    domain: &'static str,
    domain_id: u32,
    value: i64,
    name: &'static str,
    detail: &'static str,
) {
    push_inconsistent_with_evidence(
        report,
        source,
        domain,
        domain_id,
        value,
        name,
        detail,
        format!("source={source:?} raw={value}"),
    );
}

#[allow(clippy::too_many_arguments)]
fn push_inconsistent_with_evidence(
    report: &mut DiagnosticReport,
    source: &'static str,
    domain: &'static str,
    domain_id: u32,
    value: i64,
    name: &'static str,
    detail: &'static str,
    evidence: String,
) {
    report.push(Issue::new(
        Severity::Error,
        category::DIAGNOSTIC_INTERFACE,
        source,
        domain,
        domain_id,
        value,
        Some(name),
        detail,
        "retain the raw interface evidence and restart only after correcting the producing component",
        &category::DIAGNOSTIC_INTERFACE,
        evidence,
    ));
}

fn spindle_recovery_class(code: SpindleCode) -> RecoveryClass {
    match code {
        SpindleCode::None => RecoveryClass::RecheckSource,
        SpindleCode::Link
        | SpindleCode::F001
        | SpindleCode::F002
        | SpindleCode::F024
        | SpindleCode::F163
        | SpindleCode::F164
        | SpindleCode::F165
        | SpindleCode::F169
        | SpindleCode::ExplicitFrequency
        | SpindleCode::F004
        | SpindleCode::F005
        | SpindleCode::RpmLimits
        | SpindleCode::VfdFault
        | SpindleCode::DirectionChange
        | SpindleCode::Direction
        | SpindleCode::SpeedZero
        | SpindleCode::SpeedLow
        | SpindleCode::SpeedHigh
        | SpindleCode::BelowF011
        | SpindleCode::FrequencyRange
        | SpindleCode::SpeedInvalid
        | SpindleCode::CommandDisabled
        | SpindleCode::InternalState
        | SpindleCode::DirectionFeedback => RecoveryClass::ResetSpindle,
    }
}

pub fn augment(report: &mut DiagnosticReport, snapshot: ExternalDiagnosticSnapshot) {
    if snapshot.controller_fault_code != 0 {
        let evidence = snapshot.controller_fault_evidence.render();
        match ControllerFaultCode::from_wire_code(snapshot.controller_fault_code) {
            Some(code) => push_recoverable_with_evidence(
                report,
                Severity::Error,
                category::CONTROLLER_FAULT,
                "dmc2-pendant-control.fault-code",
                "dmc2_controller_fault",
                CONTROLLER_DOMAIN_ID,
                code,
                evidence,
            ),
            None => push_unknown_with_evidence(
                report,
                "dmc2-pendant-control.fault-code",
                "dmc2_controller_fault",
                CONTROLLER_DOMAIN_ID,
                i64::from(snapshot.controller_fault_code),
                "the controller exported a nonzero fault value absent from its compiled exhaustive catalog",
                "retain the controller fault-data snapshot and correct the controller/catalog mismatch before reset",
                UnknownExternalDiagnostic::CatalogMismatch,
                evidence,
            ),
        }
        if !snapshot.controller_fault_evidence.valid {
            push_inconsistent(
                report,
                "dmc2-pendant-control.fault-data-b00",
                "dmc2_controller_fault_evidence",
                CONTROLLER_DOMAIN_ID,
                i64::from(snapshot.controller_fault_code),
                "CONTROLLER_FAULT_EVIDENCE_MISSING",
                "the controller exported a nonzero fault code without marking its retained evidence valid",
            );
        }
    } else if snapshot.controller_fault_evidence.valid {
        push_inconsistent(
            report,
            "dmc2-pendant-control.fault-data-b00",
            "dmc2_controller_fault_evidence",
            CONTROLLER_DOMAIN_ID,
            0,
            "CONTROLLER_FAULT_EVIDENCE_WITHOUT_CODE",
            "the controller marked retained fault evidence valid while exporting no fault code",
        );
    }

    if snapshot.serial_bridge_fault_valid {
        let evidence = snapshot.serial_bridge_fault_evidence.render();
        match BridgeFaultCode::from_wire_code(snapshot.serial_bridge_fault_code) {
            Some(code) => push_recoverable_with_evidence(
                report,
                Severity::Error,
                category::SERIAL_BRIDGE_FAULT,
                "dmc2-pendant.bridge-fault-code",
                "dmc2_serial_bridge_fault",
                SERIAL_BRIDGE_DOMAIN_ID,
                code,
                evidence.clone(),
            ),
            None => push_unknown_with_evidence(
                report,
                "dmc2-pendant.bridge-fault-code",
                "dmc2_serial_bridge_fault",
                SERIAL_BRIDGE_DOMAIN_ID,
                i64::from(snapshot.serial_bridge_fault_code),
                "the serial bridge marked a current fault valid with a value absent from its compiled exhaustive catalog",
                "retain the serial bridge evidence pins and correct the bridge/catalog mismatch before reconnecting",
                UnknownExternalDiagnostic::CatalogMismatch,
                evidence.clone(),
            ),
        }
        let bridge_code = BridgeFaultCode::from_wire_code(snapshot.serial_bridge_fault_code);
        let nested_valid = snapshot.serial_bridge_fault_evidence.protocol_error_valid;
        if nested_valid {
            match ProtocolError::from_wire_code(
                snapshot.serial_bridge_fault_evidence.protocol_error_code,
            ) {
                Some(protocol_error) => push_recoverable_with_evidence(
                    report,
                    Severity::Error,
                    category::SERIAL_BRIDGE_FAULT,
                    "dmc2-pendant.fault-data-s00",
                    "dmc2_serial_protocol_error",
                    SERIAL_BRIDGE_DOMAIN_ID,
                    protocol_error,
                    evidence.clone(),
                ),
                None => push_unknown_with_evidence(
                    report,
                    "dmc2-pendant.fault-data-s00",
                    "dmc2_serial_protocol_error",
                    SERIAL_BRIDGE_DOMAIN_ID,
                    i64::from(snapshot.serial_bridge_fault_evidence.protocol_error_code),
                    "the serial bridge retained a nested protocol value absent from its compiled exhaustive catalog",
                    "retain the complete bridge snapshot and correct the bridge/task-monitor catalog mismatch before reconnecting",
                    UnknownExternalDiagnostic::CatalogMismatch,
                    evidence.clone(),
                ),
            }
            if bridge_code != Some(BridgeFaultCode::ProtocolRejected) {
                push_inconsistent(
                    report,
                    "dmc2-pendant.fault-data-b00",
                    "dmc2_serial_bridge_fault",
                    SERIAL_BRIDGE_DOMAIN_ID,
                    i64::from(snapshot.serial_bridge_fault_code),
                    "SERIAL_PROTOCOL_EVIDENCE_WITHOUT_PROTOCOL_FAULT",
                    "the bridge retained nested protocol evidence for a different current bridge fault",
                );
            }
        } else if bridge_code == Some(BridgeFaultCode::ProtocolRejected) {
            push_inconsistent(
                report,
                "dmc2-pendant.fault-data-b00",
                "dmc2_serial_bridge_fault",
                SERIAL_BRIDGE_DOMAIN_ID,
                i64::from(snapshot.serial_bridge_fault_code),
                "SERIAL_PROTOCOL_FAULT_WITHOUT_NESTED_EVIDENCE",
                "the bridge reported PROTOCOL_REJECTED without its required nested named protocol error",
            );
        }
    } else if snapshot.serial_bridge_fault_code != 0 {
        push_inconsistent(
            report,
            "dmc2-pendant.bridge-fault-valid",
            "dmc2_serial_bridge_fault",
            SERIAL_BRIDGE_DOMAIN_ID,
            i64::from(snapshot.serial_bridge_fault_code),
            "SERIAL_BRIDGE_FAULT_VALIDITY_INCONSISTENT",
            "the serial bridge exported a nonzero current fault code while its validity bit was clear",
        );
    }

    let h100_evidence = snapshot.h100_fault_evidence.render();
    if snapshot.spindle_fault_latched {
        match SpindleCode::from_wire_code(snapshot.spindle_fault_code)
            .filter(|code| *code != SpindleCode::None)
        {
            Some(code) => push_recoverable_with_evidence(
                report,
                Severity::Error,
                category::SPINDLE_FAULT,
                "h100-spindle.fault-code",
                "h100_spindle_fault",
                SPINDLE_FAULT_DOMAIN_ID,
                SpindleDiagnostic::Block(code),
                h100_evidence.clone(),
            ),
            None if snapshot.spindle_fault_code == SpindleCode::None.wire_code() => {
                push_inconsistent(
                    report,
                    "h100-spindle.fault-latched",
                    "h100_spindle_fault",
                    SPINDLE_FAULT_DOMAIN_ID,
                    0,
                    "SPINDLE_FAULT_LATCH_WITHOUT_CODE",
                    "the spindle fault latch was asserted without a nonzero named fault code",
                );
            }
            None => push_unknown_with_evidence(
                report,
                "h100-spindle.fault-code",
                "h100_spindle_fault",
                SPINDLE_FAULT_DOMAIN_ID,
                i64::from(snapshot.spindle_fault_code),
                "the spindle exported a latched fault value absent from its compiled exhaustive catalog",
                "retain the spindle fault-data snapshot and correct the spindle/catalog mismatch before reset",
                UnknownExternalDiagnostic::CatalogMismatch,
                h100_evidence.clone(),
            ),
        }
        if !snapshot.h100_fault_evidence.valid {
            push_inconsistent_with_evidence(
                report,
                "h100-spindle.fault-data-b00",
                "h100_spindle_fault_evidence",
                SPINDLE_FAULT_DOMAIN_ID,
                i64::from(snapshot.spindle_fault_code),
                "H100_SPINDLE_FAULT_EVIDENCE_MISSING",
                "the H100 component exported a latched spindle fault without its immutable first-fault capture",
                h100_evidence.clone(),
            );
        }
    } else if snapshot.spindle_fault_code != SpindleCode::None.wire_code() {
        push_inconsistent(
            report,
            "h100-spindle.fault-latched",
            "h100_spindle_fault",
            SPINDLE_FAULT_DOMAIN_ID,
            i64::from(snapshot.spindle_fault_code),
            "SPINDLE_FAULT_CODE_WITHOUT_LATCH",
            "the spindle exported a nonzero fault code while its fault latch was clear",
        );
    }

    if !snapshot.spindle_fault_latched
        && snapshot.spindle_block_code != SpindleCode::None.wire_code()
    {
        match SpindleCode::from_wire_code(snapshot.spindle_block_code)
            .filter(|code| *code != SpindleCode::None)
        {
            Some(code) => push_recoverable_with_evidence(
                report,
                Severity::Warning,
                category::SPINDLE_BLOCK,
                "h100-spindle.block-code",
                "h100_spindle_block",
                SPINDLE_BLOCK_DOMAIN_ID,
                SpindleDiagnostic::Block(code),
                h100_evidence.clone(),
            ),
            None => push_unknown_with_evidence(
                report,
                "h100-spindle.block-code",
                "h100_spindle_block",
                SPINDLE_BLOCK_DOMAIN_ID,
                i64::from(snapshot.spindle_block_code),
                "the spindle exported a nonzero command-block value absent from its compiled exhaustive catalog",
                "retain the spindle block inputs and correct the spindle/catalog mismatch before requesting rotation",
                UnknownExternalDiagnostic::CatalogMismatch,
                h100_evidence.clone(),
            ),
        }
    }

    if snapshot.h100_vfd_fault_code != 0 {
        match VfdFaultCode::decode(snapshot.h100_vfd_fault_code) {
            Some(code) => push_recoverable_with_evidence(
                report,
                Severity::Error,
                category::SPINDLE_FAULT,
                "h100-spindle.current-fault",
                "h100_vfd_current_fault",
                H100_VFD_DOMAIN_ID,
                SpindleDiagnostic::VfdFault(code),
                format!(
                    "source={:?} raw={} drive_display={}.{} family_slug={} phase_slug={} {}",
                    "h100-spindle.current-fault",
                    snapshot.h100_vfd_fault_code,
                    code.family.display(),
                    code.phase.display(),
                    code.family.hal_slug(),
                    code.phase.hal_slug(),
                    h100_evidence,
                ),
            ),
            None => push_unknown_with_evidence(
                report,
                "h100-spindle.current-fault",
                "h100_vfd_current_fault",
                H100_VFD_DOMAIN_ID,
                i64::from(snapshot.h100_vfd_fault_code),
                "the H100 current-fault register is nonzero but absent from the exact manual V1.8 printed-page-84 table",
                "retain the raw register, inspect the drive display, and verify the exact H100 manual code before reset",
                UnknownExternalDiagnostic::VfdCurrentFault,
                h100_evidence.clone(),
            ),
        }
    }

    let reserved_status = main_status_reserved_mask(snapshot.h100_main_status);
    if reserved_status != 0 {
        push_unknown_with_evidence(
            report,
            "h100-spindle.main-status",
            "h100_main_status_reserved_bits",
            H100_MAIN_STATUS_DOMAIN_ID,
            i64::from(reserved_status),
            "H100 holding register 0210H contains bits mapped only to manual-reserved addresses 0008H through 000FH",
            "retain the full main-status register and verify the exact drive/manual revision before continuing",
            UnknownExternalDiagnostic::CatalogMismatch,
            h100_evidence,
        );
    }
}
