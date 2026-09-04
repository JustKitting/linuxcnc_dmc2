use std::ffi::c_int;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::{AxisCode, BridgeFaultCode, MultiplierCode, ProtocolError, Snapshot};
use dmc2_diagnostics::RecoveryDisplay;
use dmc2_hal_sys as hal;

use super::pins::HalPins;
use super::registration::{create_hal, RegistrationError};

pub struct HalPublisher {
    component_id: c_int,
    pins: *mut HalPins,
    publication_generation: AtomicU32,
}

impl HalPublisher {
    pub fn new(component: &str) -> Result<Self, RegistrationError> {
        let (component_id, pins) = unsafe { create_hal(component)? };
        Ok(Self {
            component_id,
            pins,
            publication_generation: AtomicU32::new(0),
        })
    }

    pub fn publish(&self, snapshot: Snapshot, packet_age_ms: f64) {
        let pins = unsafe { &*self.pins };
        let generation = self
            .publication_generation
            .fetch_add(2, Ordering::Relaxed)
            .wrapping_add(2);
        let generation_pin = unsafe { &*(pins.snapshot_generation.cast::<AtomicU32>()) };
        unsafe {
            generation_pin.store(generation | 1, Ordering::SeqCst);
            write(pins.connected, snapshot.connected);
            write(pins.serial_fault, snapshot.serial_fault);
            write(pins.quadrature_fault, snapshot.quadrature_fault);
            write(pins.link_healthy, snapshot.link_healthy);
            write(pins.heartbeat, snapshot.heartbeat);
            write(pins.estop_pressed, snapshot.estop_pressed);
            write(pins.deadman_held, snapshot.deadman_held);
            write(pins.selector_valid, snapshot.selector_valid);
            for (pointer, known) in pins.axis.iter().zip(AxisCode::ALL) {
                write(*pointer, snapshot.axis == known);
            }
            for (pointer, known) in pins.multiplier.iter().zip(MultiplierCode::ALL) {
                write(*pointer, snapshot.multiplier == known);
            }
            write(pins.axis_code, snapshot.axis.wire_code());
            write(pins.multiplier_code, snapshot.multiplier.wire_code());
            write(pins.latest_detent, snapshot.latest_detent);
            write(pins.detent_count, snapshot.detent_count);
            write(pins.transition_count, snapshot.transition_count);
            write(pins.quadrature_errors, snapshot.quadrature_errors);
            write(pins.sequence, snapshot.sequence);
            write(pins.milliseconds, snapshot.milliseconds);
            write(pins.protocol_errors, snapshot.protocol_errors);
            write(pins.dropped_packets, snapshot.dropped_packets);
            write(pins.timeouts, snapshot.timeouts);
            write(pins.packet_age_ms, packet_age_ms);
            let bridge_fault = snapshot.current_fault;
            let bridge_fault_code = bridge_fault.map(|record| record.code);
            write(pins.bridge_fault_valid, bridge_fault.is_some());
            for (index, known) in BridgeFaultCode::ALL.iter().copied().enumerate() {
                write(
                    pins.bridge_fault_kind[index],
                    bridge_fault_code == Some(known),
                );
            }
            write(
                pins.bridge_fault_code,
                bridge_fault_code.map_or(0, BridgeFaultCode::wire_code),
            );
            let bridge_evidence = bridge_fault.map(|record| record.evidence);
            let protocol_error = bridge_evidence.and_then(|value| value.protocol_error);
            write(
                pins.bridge_fault_protocol_error_valid,
                protocol_error.is_some(),
            );
            for (index, known) in ProtocolError::ALL.iter().copied().enumerate() {
                write(
                    pins.bridge_fault_protocol_error_kind[index],
                    protocol_error == Some(known),
                );
            }
            write(
                pins.bridge_fault_protocol_error_code,
                protocol_error.map_or(0, ProtocolError::wire_code),
            );
            write_optional_u32(
                pins.bridge_fault_line_bytes_valid,
                pins.bridge_fault_line_bytes,
                bridge_evidence.and_then(|value| value.line_bytes),
            );
            write_optional_u32(
                pins.bridge_fault_previous_sequence_valid,
                pins.bridge_fault_previous_sequence,
                bridge_evidence.and_then(|value| value.previous_sequence),
            );
            write_optional_u32(
                pins.bridge_fault_observed_sequence_valid,
                pins.bridge_fault_observed_sequence,
                bridge_evidence.and_then(|value| value.observed_sequence),
            );
            write_optional_float(
                pins.bridge_fault_packet_age_valid,
                pins.bridge_fault_packet_age_ms,
                bridge_evidence
                    .and_then(|value| value.packet_age_ns)
                    .map(nanoseconds_to_milliseconds),
            );
            write_optional_float(
                pins.bridge_fault_timeout_valid,
                pins.bridge_fault_timeout_ms,
                bridge_evidence
                    .and_then(|value| value.timeout_ns)
                    .map(nanoseconds_to_milliseconds),
            );
            write_optional_u32(
                pins.bridge_fault_previous_quadrature_errors_valid,
                pins.bridge_fault_previous_quadrature_errors,
                bridge_evidence.and_then(|value| value.previous_quadrature_errors),
            );
            write_optional_u32(
                pins.bridge_fault_observed_quadrature_errors_valid,
                pins.bridge_fault_observed_quadrature_errors,
                bridge_evidence.and_then(|value| value.observed_quadrature_errors),
            );
            write_optional_s32(
                pins.bridge_fault_operating_system_error_valid,
                pins.bridge_fault_operating_system_error,
                bridge_evidence.and_then(|value| value.operating_system_error),
            );
            write_optional_i64_bits(
                pins.bridge_fault_transport_contract_result_valid,
                pins.bridge_fault_transport_contract_result_low,
                pins.bridge_fault_transport_contract_result_high,
                bridge_evidence.and_then(|value| value.transport_contract_result),
            );
            let error_record = snapshot.last_protocol_error;
            let error_code = error_record.map(|record| record.code);
            write(pins.last_protocol_error_valid, error_record.is_some());
            for (index, known) in ProtocolError::ALL.iter().copied().enumerate() {
                write(
                    pins.last_protocol_error_kind[index],
                    error_code == Some(known),
                );
            }
            write(
                pins.last_protocol_error_code,
                error_code.map_or(0, ProtocolError::wire_code),
            );
            let evidence = error_record.map(|record| record.evidence);
            write_optional_u32(
                pins.last_protocol_error_line_bytes_valid,
                pins.last_protocol_error_line_bytes,
                evidence.and_then(|value| value.line_bytes),
            );
            write_optional_u32(
                pins.last_protocol_error_previous_sequence_valid,
                pins.last_protocol_error_previous_sequence,
                evidence.and_then(|value| value.previous_sequence),
            );
            write_optional_u32(
                pins.last_protocol_error_observed_sequence_valid,
                pins.last_protocol_error_observed_sequence,
                evidence.and_then(|value| value.observed_sequence),
            );
            generation_pin.store(generation, Ordering::SeqCst);
        }
    }
}

impl Drop for HalPublisher {
    fn drop(&mut self) {
        if let Err(error) = hal::HalCall::Exit.classify(unsafe { hal::hal_exit(self.component_id) })
        {
            eprintln!(
                "dmc2-serial-bridge: hal_exit failed: {}",
                RecoveryDisplay(&error)
            );
        }
    }
}

unsafe fn write<T: Copy>(pointer: *mut T, value: T) {
    unsafe { ptr::write_volatile(pointer, value) };
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

unsafe fn write_optional_i64_bits(
    valid: *mut hal::hal_bit_t,
    low: *mut hal::hal_u32_t,
    high: *mut hal::hal_u32_t,
    source: Option<i64>,
) {
    let bits = source.unwrap_or(0) as u64;
    unsafe {
        write(valid, source.is_some());
        write(low, bits as u32);
        write(high, (bits >> 32) as u32);
    }
}

const fn nanoseconds_to_milliseconds(value: u64) -> f64 {
    value as f64 / 1_000_000.0
}
