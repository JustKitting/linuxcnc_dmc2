use std::ffi::c_int;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

use dmc2_diagnostics::RecoveryDisplay;
use dmc2_hal_sys as hal;
use dmc2_linuxcnc_interface::{CMS_STATUS, NML_ERROR, TASK_INTERP, TASK_MODE, TRAJ_MODE};

use crate::application::diagnostic_journal::DiagnosticJournal;
use crate::application::diagnostic_state::DiagnosticState;
use crate::application::nml::{PollCodes, TransportStatus};
use crate::diagnostics::{
    self, ControllerFaultEvidenceSnapshot, DiagnosticReport, ExternalDiagnosticSnapshot,
    H100FaultEvidenceSnapshot, SerialBridgeFaultEvidenceSnapshot,
};
use crate::snapshot::NativeSnapshot;

use super::pins::HalPins;
use super::registration::create_hal;
use super::PublisherError;

const CONTROLLER_SNAPSHOT_READ_ATTEMPTS: usize = 8;
const SERIAL_BRIDGE_SNAPSHOT_READ_ATTEMPTS: usize = 8;
const H100_SNAPSHOT_READ_ATTEMPTS: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ControllerFaultSnapshot {
    code: i32,
    evidence: ControllerFaultEvidenceSnapshot,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct SerialBridgeFaultSnapshot {
    valid: bool,
    code: i32,
    evidence: SerialBridgeFaultEvidenceSnapshot,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct H100DiagnosticSnapshot {
    fault_latched: bool,
    fault_code: u32,
    block_code: u32,
    vfd_fault_code: u32,
    main_status: u32,
    evidence: H100FaultEvidenceSnapshot,
}

unsafe fn read_controller_fault_evidence(
    pins: &HalPins,
    snapshot_generation: u32,
) -> ControllerFaultEvidenceSnapshot {
    let evidence = &pins.controller_fault_evidence;
    unsafe {
        ControllerFaultEvidenceSnapshot {
            snapshot_generation,
            valid: ptr::read_volatile(evidence.valid),
            axis_valid: ptr::read_volatile(evidence.axis_valid),
            axis: evidence.axis.map(|pointer| ptr::read_volatile(pointer)),
            motor_valid: ptr::read_volatile(evidence.motor_valid),
            motor: ptr::read_volatile(evidence.motor),
            start_count_valid: ptr::read_volatile(evidence.start_count_valid),
            start_count: ptr::read_volatile(evidence.start_count),
            target_count_valid: ptr::read_volatile(evidence.target_count_valid),
            target_count: ptr::read_volatile(evidence.target_count),
            observed_count_valid: ptr::read_volatile(evidence.observed_count_valid),
            observed_count: ptr::read_volatile(evidence.observed_count),
            target_position_valid: ptr::read_volatile(evidence.target_position_valid),
            target_position_pulses: ptr::read_volatile(evidence.target_position_pulses),
            observed_position_valid: ptr::read_volatile(evidence.observed_position_valid),
            observed_position_pulses: ptr::read_volatile(evidence.observed_position_pulses),
            position_error_valid: ptr::read_volatile(evidence.position_error_valid),
            position_error_pulses: ptr::read_volatile(evidence.position_error_pulses),
            counts_by_motor: evidence
                .counts_by_motor
                .map(|pointer| ptr::read_volatile(pointer)),
            position_feedback_by_motor: evidence
                .position_feedback_by_motor
                .map(|pointer| ptr::read_volatile(pointer)),
            raw_limit_mask: ptr::read_volatile(evidence.raw_limit_mask),
            safety_limit_mask: ptr::read_volatile(evidence.safety_limit_mask),
            expected_limit_mask_valid: ptr::read_volatile(evidence.expected_limit_mask_valid),
            expected_limit_mask: ptr::read_volatile(evidence.expected_limit_mask),
            elapsed_valid: ptr::read_volatile(evidence.elapsed_valid),
            elapsed_seconds: ptr::read_volatile(evidence.elapsed_seconds),
            timeout_valid: ptr::read_volatile(evidence.timeout_valid),
            timeout_seconds: ptr::read_volatile(evidence.timeout_seconds),
            link_connected: ptr::read_volatile(evidence.link_connected),
            serial_fault: ptr::read_volatile(evidence.serial_fault),
            quadrature_fault: ptr::read_volatile(evidence.quadrature_fault),
            pendant_estop_pressed: ptr::read_volatile(evidence.pendant_estop_pressed),
            machine_on: ptr::read_volatile(evidence.machine_on),
            machine_estopped: ptr::read_volatile(evidence.machine_estopped),
            manual_mode: ptr::read_volatile(evidence.manual_mode),
            joint_mode: ptr::read_volatile(evidence.joint_mode),
            teleop_mode: ptr::read_volatile(evidence.teleop_mode),
            interp_idle: ptr::read_volatile(evidence.interp_idle),
            homed_mask: ptr::read_volatile(evidence.homed_mask),
            homing_mask: ptr::read_volatile(evidence.homing_mask),
            stopped_mask: ptr::read_volatile(evidence.stopped_mask),
            motion_command_ready: ptr::read_volatile(evidence.motion_command_ready),
            motion_enabled: ptr::read_volatile(evidence.motion_enabled),
            motion_teleop_mode: ptr::read_volatile(evidence.motion_teleop_mode),
            motion_coord_mode: ptr::read_volatile(evidence.motion_coord_mode),
            motion_in_position: ptr::read_volatile(evidence.motion_in_position),
            motion_jog_active: ptr::read_volatile(evidence.motion_jog_active),
            consumer_active_seen: ptr::read_volatile(evidence.consumer_active_seen),
            feedback_progress_seen: ptr::read_volatile(evidence.feedback_progress_seen),
            axis_wheel_jog_active_mask: ptr::read_volatile(evidence.axis_wheel_jog_active_mask),
            joint_wheel_jog_active_mask: ptr::read_volatile(evidence.joint_wheel_jog_active_mask),
            joint_in_position_mask: ptr::read_volatile(evidence.joint_in_position_mask),
            task_heartbeat_age_valid: ptr::read_volatile(evidence.task_heartbeat_age_valid),
            task_heartbeat_age_seconds: ptr::read_volatile(evidence.task_heartbeat_age_seconds),
            pendant_packet_age_valid: ptr::read_volatile(evidence.pendant_packet_age_valid),
            pendant_packet_age_seconds: ptr::read_volatile(evidence.pendant_packet_age_seconds),
            supervisor_phase: ptr::read_volatile(evidence.supervisor_phase),
            mesa_phase_valid: ptr::read_volatile(evidence.mesa_phase_valid),
            mesa_phase: ptr::read_volatile(evidence.mesa_phase),
            controller_watchdog_phase_valid: ptr::read_volatile(
                evidence.controller_watchdog_phase_valid,
            ),
            controller_watchdog_phase: ptr::read_volatile(evidence.controller_watchdog_phase),
        }
    }
}

unsafe fn read_controller_fault_snapshot(pins: &HalPins) -> Option<ControllerFaultSnapshot> {
    let generation = unsafe {
        &*(pins
            .controller_fault_snapshot_generation_in
            .cast::<AtomicU32>())
    };
    for _ in 0..CONTROLLER_SNAPSHOT_READ_ATTEMPTS {
        let first = generation.load(Ordering::SeqCst);
        if first & 1 != 0 {
            continue;
        }
        let code = unsafe { ptr::read_volatile(pins.controller_fault_code_in) };
        let evidence = unsafe { read_controller_fault_evidence(pins, first) };
        let second = generation.load(Ordering::SeqCst);
        if first == second && second & 1 == 0 {
            return Some(ControllerFaultSnapshot { code, evidence });
        }
    }
    None
}

unsafe fn read_serial_bridge_fault_evidence(
    pins: &HalPins,
    snapshot_generation: u32,
) -> SerialBridgeFaultEvidenceSnapshot {
    let evidence = &pins.serial_bridge_fault_evidence;
    let contract_bits =
        u64::from(unsafe { ptr::read_volatile(evidence.transport_contract_result_low) })
            | (u64::from(unsafe { ptr::read_volatile(evidence.transport_contract_result_high) })
                << 32);
    unsafe {
        SerialBridgeFaultEvidenceSnapshot {
            snapshot_generation,
            protocol_error_valid: ptr::read_volatile(evidence.protocol_error_valid),
            protocol_error_code: ptr::read_volatile(evidence.protocol_error_code),
            line_bytes_valid: ptr::read_volatile(evidence.line_bytes_valid),
            line_bytes: ptr::read_volatile(evidence.line_bytes),
            previous_sequence_valid: ptr::read_volatile(evidence.previous_sequence_valid),
            previous_sequence: ptr::read_volatile(evidence.previous_sequence),
            observed_sequence_valid: ptr::read_volatile(evidence.observed_sequence_valid),
            observed_sequence: ptr::read_volatile(evidence.observed_sequence),
            packet_age_valid: ptr::read_volatile(evidence.packet_age_valid),
            packet_age_ms: ptr::read_volatile(evidence.packet_age_ms),
            timeout_valid: ptr::read_volatile(evidence.timeout_valid),
            timeout_ms: ptr::read_volatile(evidence.timeout_ms),
            previous_quadrature_errors_valid: ptr::read_volatile(
                evidence.previous_quadrature_errors_valid,
            ),
            previous_quadrature_errors: ptr::read_volatile(evidence.previous_quadrature_errors),
            observed_quadrature_errors_valid: ptr::read_volatile(
                evidence.observed_quadrature_errors_valid,
            ),
            observed_quadrature_errors: ptr::read_volatile(evidence.observed_quadrature_errors),
            operating_system_error_valid: ptr::read_volatile(evidence.operating_system_error_valid),
            operating_system_error: ptr::read_volatile(evidence.operating_system_error),
            transport_contract_result_valid: ptr::read_volatile(
                evidence.transport_contract_result_valid,
            ),
            transport_contract_result: contract_bits as i64,
        }
    }
}

unsafe fn read_serial_bridge_fault_snapshot(pins: &HalPins) -> Option<SerialBridgeFaultSnapshot> {
    let generation = unsafe {
        &*(pins
            .serial_bridge_snapshot_generation_in
            .cast::<AtomicU32>())
    };
    for _ in 0..SERIAL_BRIDGE_SNAPSHOT_READ_ATTEMPTS {
        let first = generation.load(Ordering::SeqCst);
        if first & 1 != 0 {
            continue;
        }
        let valid = unsafe { ptr::read_volatile(pins.serial_bridge_fault_valid_in) };
        let code = unsafe { ptr::read_volatile(pins.serial_bridge_fault_code_in) };
        let evidence = unsafe { read_serial_bridge_fault_evidence(pins, first) };
        let second = generation.load(Ordering::SeqCst);
        if first == second && second & 1 == 0 {
            return Some(SerialBridgeFaultSnapshot {
                valid,
                code,
                evidence,
            });
        }
    }
    None
}

unsafe fn read_h100_fault_evidence(
    pins: &HalPins,
    snapshot_generation: u32,
) -> H100FaultEvidenceSnapshot {
    let evidence = &pins.h100_fault_evidence;
    unsafe {
        H100FaultEvidenceSnapshot {
            snapshot_generation,
            valid: ptr::read_volatile(evidence.valid),
            state_before: ptr::read_volatile(evidence.state_before),
            context_fault_latched_before: ptr::read_volatile(evidence.context_fault_latched_before),
            context_fault_record_present_before: ptr::read_volatile(
                evidence.context_fault_record_present_before,
            ),
            context_fault_code_before: ptr::read_volatile(evidence.context_fault_code_before),
            context_fault_record_code_before: ptr::read_volatile(
                evidence.context_fault_record_code_before,
            ),
            machine_enabled: ptr::read_volatile(evidence.machine_enabled),
            run_request: ptr::read_volatile(evidence.run_request),
            forward_request: ptr::read_volatile(evidence.forward_request),
            reverse_request: ptr::read_volatile(evidence.reverse_request),
            reset: ptr::read_volatile(evidence.reset),
            link_fault: ptr::read_volatile(evidence.link_fault),
            command_disabled: ptr::read_volatile(evidence.command_disabled),
            speed_command_rpm: ptr::read_volatile(evidence.speed_command_rpm),
            control_mode_f001: ptr::read_volatile(evidence.control_mode_f001),
            frequency_source_f002: ptr::read_volatile(evidence.frequency_source_f002),
            reference_f004_centihz: ptr::read_volatile(evidence.reference_f004_centihz),
            maximum_f005_centihz: ptr::read_volatile(evidence.maximum_f005_centihz),
            lower_limit_f011_centihz: ptr::read_volatile(evidence.lower_limit_f011_centihz),
            panel_stop_f024: ptr::read_volatile(evidence.panel_stop_f024),
            slave_address_f163: ptr::read_volatile(evidence.slave_address_f163),
            baud_selector_f164: ptr::read_volatile(evidence.baud_selector_f164),
            data_mode_f165: ptr::read_volatile(evidence.data_mode_f165),
            frequency_decimals_f169: ptr::read_volatile(evidence.frequency_decimals_f169),
            output_frequency_decihz: ptr::read_volatile(evidence.output_frequency_decihz),
            current_vfd_fault: ptr::read_volatile(evidence.current_vfd_fault),
            main_status: ptr::read_volatile(evidence.main_status),
            given_frequency_readback: ptr::read_volatile(evidence.given_frequency_readback),
            expected_reference_f004_centihz: ptr::read_volatile(
                evidence.expected_reference_f004_centihz,
            ),
            expected_maximum_f005_centihz: ptr::read_volatile(
                evidence.expected_maximum_f005_centihz,
            ),
            calculated_frequency_register: ptr::read_volatile(
                evidence.calculated_frequency_register,
            ),
            rated_rpm: ptr::read_volatile(evidence.rated_rpm),
            minimum_rpm: ptr::read_volatile(evidence.minimum_rpm),
            maximum_rpm: ptr::read_volatile(evidence.maximum_rpm),
            at_speed_tolerance_hz: ptr::read_volatile(evidence.at_speed_tolerance_hz),
            calculated_frequency_hz: ptr::read_volatile(evidence.calculated_frequency_hz),
        }
    }
}

unsafe fn read_h100_diagnostic_snapshot(pins: &HalPins) -> Option<H100DiagnosticSnapshot> {
    let generation = unsafe {
        &*(pins
            .h100_diagnostic_snapshot_generation_in
            .cast::<AtomicU32>())
    };
    for _ in 0..H100_SNAPSHOT_READ_ATTEMPTS {
        let first = generation.load(Ordering::SeqCst);
        if first & 1 != 0 {
            continue;
        }
        let snapshot = H100DiagnosticSnapshot {
            fault_latched: unsafe { ptr::read_volatile(pins.spindle_fault_latched_in) },
            fault_code: unsafe { ptr::read_volatile(pins.spindle_fault_code_in) },
            block_code: unsafe { ptr::read_volatile(pins.spindle_block_code_in) },
            vfd_fault_code: unsafe { ptr::read_volatile(pins.h100_vfd_fault_code_in) },
            main_status: unsafe { ptr::read_volatile(pins.h100_main_status_in) },
            evidence: unsafe { read_h100_fault_evidence(pins, first) },
        };
        let second = generation.load(Ordering::SeqCst);
        if first == second && second & 1 == 0 {
            return Some(snapshot);
        }
    }
    None
}

pub(in crate::application) struct HalPublisher {
    component_id: c_int,
    pins: *mut HalPins,
    publications: AtomicU32,
    controller_fault_snapshot: ControllerFaultSnapshot,
    serial_bridge_fault_snapshot: SerialBridgeFaultSnapshot,
    h100_diagnostic_snapshot: H100DiagnosticSnapshot,
    diagnostic_journal: DiagnosticJournal,
}

impl HalPublisher {
    pub(in crate::application) fn new(
        component: &str,
        diagnostic_journal_path: &Path,
        codes: PollCodes,
    ) -> Result<Self, PublisherError> {
        let diagnostic_journal = DiagnosticJournal::create(diagnostic_journal_path)?;
        let (component_id, pins) = unsafe { create_hal(component)? };
        let publisher = Self {
            component_id,
            pins,
            publications: AtomicU32::new(0),
            controller_fault_snapshot: ControllerFaultSnapshot::default(),
            serial_bridge_fault_snapshot: SerialBridgeFaultSnapshot::default(),
            h100_diagnostic_snapshot: H100DiagnosticSnapshot::default(),
            diagnostic_journal,
        };
        publisher.publish_initial_transport(codes);
        Ok(publisher)
    }

    fn publish_initial_transport(&self, codes: PollCodes) {
        let pins = unsafe { &*self.pins };
        unsafe {
            ptr::write_volatile(pins.nml_error_code, codes.invalid_configuration);
            ptr::write_volatile(pins.nml_error_known, true);
            ptr::write_volatile(pins.nml_error_unknown, false);
            for (entry, pointer) in NML_ERROR.codes.iter().zip(pins.nml_error_kind) {
                ptr::write_volatile(
                    pointer,
                    entry.code == i64::from(codes.invalid_configuration),
                );
            }
            ptr::write_volatile(pins.cms_status_code, codes.cms_status_not_set);
            ptr::write_volatile(pins.cms_status_known, true);
            ptr::write_volatile(pins.cms_status_unknown, false);
            for (entry, pointer) in CMS_STATUS.codes.iter().zip(pins.cms_status_kind) {
                ptr::write_volatile(pointer, entry.code == i64::from(codes.cms_status_not_set));
            }
        }
    }

    pub(in crate::application) fn increment_poll_errors(&self) {
        let pins = unsafe { &*self.pins };
        unsafe {
            let errors = ptr::read_volatile(pins.poll_errors).wrapping_add(1);
            ptr::write_volatile(pins.poll_errors, errors);
        }
    }

    pub(in crate::application) fn publish(
        &mut self,
        snapshot: NativeSnapshot,
        connected: bool,
        fault: bool,
        transport: TransportStatus,
        linuxcnc_diagnostics: &DiagnosticReport,
        diagnostic_state: &mut DiagnosticState,
    ) -> Result<(), PublisherError> {
        let pins = unsafe { &*self.pins };
        if let Some(snapshot) = unsafe { read_controller_fault_snapshot(pins) } {
            self.controller_fault_snapshot = snapshot;
        }
        if let Some(snapshot) = unsafe { read_serial_bridge_fault_snapshot(pins) } {
            self.serial_bridge_fault_snapshot = snapshot;
        }
        if let Some(snapshot) = unsafe { read_h100_diagnostic_snapshot(pins) } {
            self.h100_diagnostic_snapshot = snapshot;
        }
        let mut diagnostics = linuxcnc_diagnostics.clone();
        diagnostics::augment_external(
            &mut diagnostics,
            ExternalDiagnosticSnapshot {
                controller_fault_code: self.controller_fault_snapshot.code,
                controller_fault_evidence: self.controller_fault_snapshot.evidence,
                serial_bridge_fault_valid: self.serial_bridge_fault_snapshot.valid,
                serial_bridge_fault_code: self.serial_bridge_fault_snapshot.code,
                serial_bridge_fault_evidence: self.serial_bridge_fault_snapshot.evidence,
                spindle_fault_latched: self.h100_diagnostic_snapshot.fault_latched,
                spindle_fault_code: self.h100_diagnostic_snapshot.fault_code,
                spindle_block_code: self.h100_diagnostic_snapshot.block_code,
                h100_vfd_fault_code: self.h100_diagnostic_snapshot.vfd_fault_code,
                h100_main_status: self.h100_diagnostic_snapshot.main_status,
                h100_fault_evidence: self.h100_diagnostic_snapshot.evidence,
            },
        );
        let clear_latched = unsafe { ptr::read_volatile(pins.clear_latched) };
        let transitions = diagnostic_state.update(&diagnostics, clear_latched);
        for transition in &transitions.events {
            let sequence = self.diagnostic_journal.append(transition)?;
            diagnostic_state.record_journal_sequence(sequence);
        }
        let publications = self
            .publications
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        let generation = publications.wrapping_shl(1);
        let generation_pin = unsafe { &*(pins.snapshot_generation.cast::<AtomicU32>()) };
        unsafe {
            generation_pin.store(generation | 1, Ordering::SeqCst);
            ptr::write_volatile(pins.task_heartbeat, snapshot.task.heartbeat);
            ptr::write_volatile(pins.machine_on, snapshot.trajectory.enabled != 0);
            ptr::write_volatile(pins.estopped, snapshot.io.aux.estop != 0);
            ptr::write_volatile(
                pins.manual_mode,
                TASK_MODE.lookup(i64::from(snapshot.task.mode)) == Some("EMC_TASK_MODE_MANUAL"),
            );
            ptr::write_volatile(
                pins.joint_mode,
                TRAJ_MODE.lookup(i64::from(snapshot.trajectory.mode)) == Some("EMC_TRAJ_MODE_FREE"),
            );
            ptr::write_volatile(
                pins.teleop_mode,
                TRAJ_MODE.lookup(i64::from(snapshot.trajectory.mode))
                    == Some("EMC_TRAJ_MODE_TELEOP"),
            );
            ptr::write_volatile(
                pins.interp_idle,
                TASK_INTERP.lookup(i64::from(snapshot.task.interp_state))
                    == Some("EMC_TASK_INTERP_IDLE"),
            );
            for index in 0..3 {
                ptr::write_volatile(pins.homed[index], snapshot.joints[index].homed != 0);
                ptr::write_volatile(pins.homing[index], snapshot.joints[index].homing != 0);
                ptr::write_volatile(pins.axis_stopped[index], snapshot.axes[index].stopped != 0);
            }
            ptr::write_volatile(pins.connected, connected);
            ptr::write_volatile(pins.fault, fault);
            ptr::write_volatile(pins.nml_error_code, transport.nml_error);
            let nml_known = NML_ERROR.lookup(i64::from(transport.nml_error)).is_some();
            ptr::write_volatile(pins.nml_error_known, nml_known);
            ptr::write_volatile(pins.nml_error_unknown, !nml_known);
            for (entry, pointer) in NML_ERROR.codes.iter().zip(pins.nml_error_kind) {
                ptr::write_volatile(pointer, entry.code == i64::from(transport.nml_error));
            }
            ptr::write_volatile(pins.cms_status_code, transport.cms_status);
            let cms_known = CMS_STATUS.lookup(i64::from(transport.cms_status)).is_some();
            ptr::write_volatile(pins.cms_status_known, cms_known);
            ptr::write_volatile(pins.cms_status_unknown, !cms_known);
            for (entry, pointer) in CMS_STATUS.codes.iter().zip(pins.cms_status_kind) {
                ptr::write_volatile(pointer, entry.code == i64::from(transport.cms_status));
            }
            ptr::write_volatile(pins.linuxcnc_error_active, diagnostics.error_active());
            ptr::write_volatile(pins.linuxcnc_warning_active, diagnostics.warning_active());
            ptr::write_volatile(pins.unknown_code_active, diagnostics.unknown_code_active());
            ptr::write_volatile(
                pins.active_error_mask_low,
                diagnostics.active_error_mask as u32,
            );
            ptr::write_volatile(
                pins.active_error_mask_high,
                (diagnostics.active_error_mask >> 32) as u32,
            );
            ptr::write_volatile(
                pins.active_warning_mask_low,
                diagnostics.active_warning_mask as u32,
            );
            ptr::write_volatile(
                pins.active_warning_mask_high,
                (diagnostics.active_warning_mask >> 32) as u32,
            );
            ptr::write_volatile(
                pins.latched_error_mask_low,
                diagnostic_state.latched_error_mask as u32,
            );
            ptr::write_volatile(
                pins.latched_error_mask_high,
                (diagnostic_state.latched_error_mask >> 32) as u32,
            );
            ptr::write_volatile(
                pins.latched_warning_mask_low,
                diagnostic_state.latched_warning_mask as u32,
            );
            ptr::write_volatile(
                pins.latched_warning_mask_high,
                (diagnostic_state.latched_warning_mask >> 32) as u32,
            );
            ptr::write_volatile(
                pins.unknown_domain_mask_low,
                diagnostics.unknown_domain_mask as u32,
            );
            ptr::write_volatile(
                pins.unknown_domain_mask_high,
                (diagnostics.unknown_domain_mask >> 32) as u32,
            );
            ptr::write_volatile(
                pins.diagnostic_count,
                diagnostics.issue_count().try_into().unwrap_or(u32::MAX),
            );
            ptr::write_volatile(pins.unknown_code_count, diagnostics.unknown_code_count());
            ptr::write_volatile(pins.diagnostic_transitions, diagnostic_state.transitions);
            ptr::write_volatile(pins.latest_code_domain, diagnostic_state.latest_code_domain);
            ptr::write_volatile(pins.latest_code_low, diagnostic_state.latest_code_low);
            ptr::write_volatile(pins.latest_code_high, diagnostic_state.latest_code_high);
            ptr::write_volatile(pins.latest_severity, diagnostic_state.latest_severity);
            ptr::write_volatile(pins.latest_action, diagnostic_state.latest_action);
            ptr::write_volatile(pins.latest_code_known, diagnostic_state.latest_code_known);
            ptr::write_volatile(
                pins.latest_code_unknown,
                diagnostic_state.latest_action != 0 && !diagnostic_state.latest_code_known,
            );
            ptr::write_volatile(
                pins.latest_journal_sequence_low,
                diagnostic_state.latest_journal_sequence as u32,
            );
            ptr::write_volatile(
                pins.latest_journal_sequence_high,
                (diagnostic_state.latest_journal_sequence >> 32) as u32,
            );
            ptr::write_volatile(pins.publications, publications);
            generation_pin.store(generation, Ordering::SeqCst);
        }
        Ok(())
    }
}

impl Drop for HalPublisher {
    fn drop(&mut self) {
        if let Err(error) = hal::HalCall::Exit.classify(unsafe { hal::hal_exit(self.component_id) })
        {
            eprintln!(
                "dmc2-task-monitor: hal_exit failed: {}",
                RecoveryDisplay(&error)
            );
        }
    }
}
