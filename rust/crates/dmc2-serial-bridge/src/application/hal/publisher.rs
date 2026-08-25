use std::ffi::c_int;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

use dmc2_hal_sys as hal;
use dmc2_serial_bridge::{AxisCode, MultiplierCode, Snapshot};

use super::pins::HalPins;
use super::registration::create_hal;

pub(in crate::application) struct HalPublisher {
    component_id: c_int,
    pins: *mut HalPins,
    publication_generation: AtomicU32,
}

impl HalPublisher {
    pub(in crate::application) fn new(component: &str) -> Result<Self, String> {
        let (component_id, pins) = unsafe { create_hal(component)? };
        Ok(Self {
            component_id,
            pins,
            publication_generation: AtomicU32::new(0),
        })
    }

    pub(in crate::application) fn publish(&self, snapshot: Snapshot, packet_age_ms: f64) {
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
            for (pointer, active) in pins.axis.iter().zip([
                snapshot.axis == AxisCode::X,
                snapshot.axis == AxisCode::Y,
                snapshot.axis == AxisCode::Z,
                snapshot.axis == AxisCode::Axis4,
                snapshot.axis == AxisCode::Axis5,
                snapshot.axis == AxisCode::Off,
                snapshot.axis == AxisCode::Invalid,
            ]) {
                write(*pointer, active);
            }
            for (pointer, active) in pins.multiplier.iter().zip([
                snapshot.multiplier == MultiplierCode::X1,
                snapshot.multiplier == MultiplierCode::X10,
                snapshot.multiplier == MultiplierCode::X100,
                snapshot.multiplier == MultiplierCode::Off,
                snapshot.multiplier == MultiplierCode::Invalid,
            ]) {
                write(*pointer, active);
            }
            write(pins.axis_code, snapshot.axis as i32);
            write(pins.multiplier_code, snapshot.multiplier as i32);
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
            generation_pin.store(generation, Ordering::SeqCst);
        }
    }

    #[cfg(test)]
    pub(super) fn test_pins(&self) -> &HalPins {
        unsafe { &*self.pins }
    }
}

impl Drop for HalPublisher {
    fn drop(&mut self) {
        let result = unsafe { hal::hal_exit(self.component_id) };
        if result != 0 {
            eprintln!("dmc2-serial-bridge: hal_exit failed: {result}");
        }
    }
}

unsafe fn write<T: Copy>(pointer: *mut T, value: T) {
    unsafe { ptr::write_volatile(pointer, value) };
}
