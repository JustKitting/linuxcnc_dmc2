use std::{mem, ptr};

use crate::application::nml::{required_cms_status, required_nml_error};
use crate::snapshot::{NativeSnapshot, SNAPSHOT_ABI_VERSION};
use dmc2_hal_sys as hal;
use dmc2_linuxcnc_interface::{CMS_STATUS, NML_ERROR};

use super::pins::HalPins;

unsafe fn publish_initial_safe(pins: &HalPins) {
    unsafe {
        pins.initialize_zero();
        ptr::write_volatile(pins.snapshot_generation, 1);
        ptr::write_volatile(pins.fault, true);
        let initial_nml_error = required_nml_error("NML_INVALID_CONFIGURATION");
        ptr::write_volatile(pins.nml_error_code, initial_nml_error);
        ptr::write_volatile(pins.nml_error_known, true);
        for (entry, pointer) in NML_ERROR.codes.iter().zip(pins.nml_error_kind) {
            ptr::write_volatile(pointer, entry.code == i64::from(initial_nml_error));
        }
        let initial_cms_status = required_cms_status("CMS_STATUS_NOT_SET");
        ptr::write_volatile(pins.cms_status_code, initial_cms_status);
        ptr::write_volatile(pins.cms_status_known, true);
        for (entry, pointer) in CMS_STATUS.codes.iter().zip(pins.cms_status_kind) {
            ptr::write_volatile(pointer, entry.code == i64::from(initial_cms_status));
        }
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
