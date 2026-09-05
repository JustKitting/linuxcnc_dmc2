use dmc2_hal_sys as hal;

use super::pins::HalPins;

unsafe fn publish_initial_safe(pins: &HalPins) {
    unsafe {
        pins.initialize_zero();
    }
}

hal::userspace_hal_component! {
    error pub RegistrationError;
    register pub(super) register_pin;
    create pub(super) create_hal;
    pins HalPins;
    initialize publish_initial_safe;
}
