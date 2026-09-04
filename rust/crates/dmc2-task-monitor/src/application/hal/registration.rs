use std::{mem, ptr};

use crate::snapshot::{NativeSnapshot, SNAPSHOT_ABI_VERSION};
use dmc2_hal_sys as hal;

use super::pins::HalPins;

unsafe fn publish_initial_safe(pins: &HalPins) {
    unsafe {
        pins.initialize_zero();
        ptr::write_volatile(pins.snapshot_generation, 1);
        ptr::write_volatile(pins.fault, true);
        // The exact typed transport values are published by `HalPublisher`
        // immediately after registration. Until then they are explicitly
        // unknown rather than guessed or looked up with a panic path.
        ptr::write_volatile(pins.nml_error_unknown, true);
        ptr::write_volatile(pins.cms_status_unknown, true);
        ptr::write_volatile(pins.linuxcnc_error_active, true);
        ptr::write_volatile(pins.latest_code_domain, -1);
        ptr::write_volatile(pins.snapshot_abi_version, SNAPSHOT_ABI_VERSION);
        ptr::write_volatile(
            pins.snapshot_struct_size,
            mem::size_of::<NativeSnapshot>() as u32,
        );
        ptr::write_volatile(pins.estopped, true);
        for pointer in pins.axis_stopped {
            ptr::write_volatile(pointer, true);
        }
        ptr::write_volatile(pins.snapshot_generation, 0);
    }
}

hal::userspace_hal_component! {
    error pub(in crate::application) RegistrationError;
    register pub(super) new_pin;
    create pub(super) create_hal;
    pins HalPins;
    initialize publish_initial_safe;
}
