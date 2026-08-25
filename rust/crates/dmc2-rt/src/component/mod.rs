//! LinuxCNC realtime component lifecycle and exported C ABI.

mod cycle;
mod hal;
mod state;

use core::ffi::{c_char, c_int, c_long, c_void};
use core::{mem, ptr};

use dmc2_hal_sys as linuxcnc_hal;

use self::cycle::update_component;
use self::hal::{publish_initial_safe, register_pins, Pins};
use self::state::ComponentState;

// `loadrt dmc2_rt` waits for this exact HAL component name. Pin and function
// names intentionally retain the stable `dmc2-pendant-control` namespace.
const COMPONENT_NAME: &[u8] = b"dmc2_rt\0";
const FUNCTION_NAME: &[u8] = b"dmc2-pendant-control.update\0";
const ENOMEM: c_int = -12;
const EINVAL: c_int = -22;

static mut COMPONENT_ID: c_int = -1;

#[cfg(not(debug_assertions))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    unsafe extern "C" {
        fn abort() -> !;
    }
    unsafe { abort() }
}

#[no_mangle]
pub extern "C" fn rtapi_app_main() -> c_int {
    let component_id = unsafe { linuxcnc_hal::hal_init(COMPONENT_NAME.as_ptr().cast::<c_char>()) };
    if component_id < 0 {
        return component_id;
    }
    unsafe { COMPONENT_ID = component_id };

    let result = (|| -> Result<(), c_int> {
        let pins =
            unsafe { linuxcnc_hal::hal_malloc(mem::size_of::<Pins>() as c_long) }.cast::<Pins>();
        if pins.is_null() {
            return Err(ENOMEM);
        }
        unsafe {
            ptr::write_bytes(pins, 0, 1);
            register_pins(pins, component_id)?;
            publish_initial_safe(&*pins);
        }

        let state = unsafe { linuxcnc_hal::hal_malloc(mem::size_of::<ComponentState>() as c_long) }
            .cast::<ComponentState>();
        if state.is_null() {
            return Err(ENOMEM);
        }
        unsafe { ptr::write(state, ComponentState::new(pins)) };

        let exported = unsafe {
            linuxcnc_hal::hal_export_funct(
                FUNCTION_NAME.as_ptr().cast::<c_char>(),
                Some(update_component),
                state.cast::<c_void>(),
                1,
                0,
                component_id,
            )
        };
        if exported != 0 {
            return Err(exported);
        }
        let ready = unsafe { linuxcnc_hal::hal_ready(component_id) };
        if ready != 0 {
            return Err(ready);
        }
        Ok(())
    })();

    match result {
        Ok(()) => 0,
        Err(error) => {
            unsafe {
                linuxcnc_hal::hal_exit(component_id);
                COMPONENT_ID = -1;
            }
            if error == 0 {
                EINVAL
            } else {
                error
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn rtapi_app_exit() {
    let component_id = unsafe { COMPONENT_ID };
    if component_id >= 0 {
        unsafe {
            linuxcnc_hal::hal_exit(component_id);
            COMPONENT_ID = -1;
        }
    }
}

#[cfg(test)]
mod tests;
