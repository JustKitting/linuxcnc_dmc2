use std::ffi::{c_int, CString};
use std::{mem, ptr};

use dmc2_hal_sys as hal;

use super::pins::{
    HalPins, AXIS_OUTPUT_PINS, BIT_OUTPUT_PINS, MULTIPLIER_OUTPUT_PINS, PACKET_AGE_PIN,
    S32_OUTPUT_PINS, SNAPSHOT_GENERATION_PIN, U32_OUTPUT_PINS,
};

fn checked_call(call: hal::HalCall, suffix: Option<&str>, raw: c_int) -> Result<c_int, String> {
    call.classify(raw).map_err(|error| match suffix {
        Some(suffix) => format!("{}({suffix}) failed: {error}", call.name()),
        None => format!("{} failed: {error}", call.name()),
    })
}

fn pin_name(component: &str, suffix: &str) -> Result<CString, String> {
    CString::new(format!("{component}.{suffix}"))
        .map_err(|_| "HAL pin name contained a NUL byte".to_owned())
}

unsafe fn bit_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::hal_bit_t,
    component_id: c_int,
) -> Result<(), String> {
    let name = pin_name(component, suffix)?;
    let result = unsafe {
        hal::hal_pin_bit_new(
            name.as_ptr(),
            hal::hal_pin_dir_t_HAL_OUT,
            pointer,
            component_id,
        )
    };
    checked_call(hal::HalCall::PinBitNew, Some(suffix), result).map(|_| ())
}

unsafe fn s32_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::hal_s32_t,
    component_id: c_int,
) -> Result<(), String> {
    let name = pin_name(component, suffix)?;
    let result = unsafe {
        hal::hal_pin_s32_new(
            name.as_ptr(),
            hal::hal_pin_dir_t_HAL_OUT,
            pointer,
            component_id,
        )
    };
    checked_call(hal::HalCall::PinS32New, Some(suffix), result).map(|_| ())
}

unsafe fn u32_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::hal_u32_t,
    component_id: c_int,
) -> Result<(), String> {
    let name = pin_name(component, suffix)?;
    let result = unsafe {
        hal::hal_pin_u32_new(
            name.as_ptr(),
            hal::hal_pin_dir_t_HAL_OUT,
            pointer,
            component_id,
        )
    };
    checked_call(hal::HalCall::PinU32New, Some(suffix), result).map(|_| ())
}

unsafe fn float_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::real_t,
    component_id: c_int,
) -> Result<(), String> {
    let name = pin_name(component, suffix)?;
    let result = unsafe {
        hal::hal_pin_float_new(
            name.as_ptr(),
            hal::hal_pin_dir_t_HAL_OUT,
            pointer,
            component_id,
        )
    };
    checked_call(hal::HalCall::PinFloatNew, Some(suffix), result).map(|_| ())
}

unsafe fn register_pins(
    component: &str,
    pins: &mut HalPins,
    component_id: c_int,
) -> Result<(), String> {
    unsafe {
        u32_pin(
            component,
            SNAPSHOT_GENERATION_PIN,
            &mut pins.snapshot_generation,
            component_id,
        )?;
        for (suffix, pointer) in BIT_OUTPUT_PINS.into_iter().zip([
            &mut pins.connected,
            &mut pins.serial_fault,
            &mut pins.quadrature_fault,
            &mut pins.link_healthy,
            &mut pins.heartbeat,
            &mut pins.estop_pressed,
            &mut pins.deadman_held,
            &mut pins.selector_valid,
        ]) {
            bit_pin(component, suffix, pointer, component_id)?;
        }
        for (index, suffix) in AXIS_OUTPUT_PINS.iter().enumerate() {
            bit_pin(component, suffix, &mut pins.axis[index], component_id)?;
        }
        for (index, suffix) in MULTIPLIER_OUTPUT_PINS.iter().enumerate() {
            bit_pin(component, suffix, &mut pins.multiplier[index], component_id)?;
        }
        for (suffix, pointer) in S32_OUTPUT_PINS.into_iter().zip([
            &mut pins.axis_code,
            &mut pins.multiplier_code,
            &mut pins.latest_detent,
            &mut pins.detent_count,
            &mut pins.transition_count,
        ]) {
            s32_pin(component, suffix, pointer, component_id)?;
        }
        for (suffix, pointer) in U32_OUTPUT_PINS.into_iter().zip([
            &mut pins.quadrature_errors,
            &mut pins.sequence,
            &mut pins.milliseconds,
            &mut pins.protocol_errors,
            &mut pins.dropped_packets,
            &mut pins.timeouts,
        ]) {
            u32_pin(component, suffix, pointer, component_id)?;
        }
        float_pin(
            component,
            PACKET_AGE_PIN,
            &mut pins.packet_age_ms,
            component_id,
        )?;
    }
    Ok(())
}

pub(super) unsafe fn create_hal(component: &str) -> Result<(c_int, *mut HalPins), String> {
    let component_name = CString::new(component)
        .map_err(|_| "HAL component name contained a NUL byte".to_owned())?;
    let component_id = checked_call(hal::HalCall::Init, None, unsafe {
        hal::hal_init(component_name.as_ptr())
    })?;

    let result = (|| {
        let pins = unsafe { hal::hal_malloc(mem::size_of::<HalPins>() as _) } as *mut HalPins;
        if pins.is_null() {
            return Err("hal_malloc for pin-pointer storage failed".to_owned());
        }
        unsafe {
            ptr::write(pins, HalPins::empty());
            register_pins(component, &mut *pins, component_id)?;
        }
        checked_call(hal::HalCall::Ready, None, unsafe {
            hal::hal_ready(component_id)
        })?;
        Ok((component_id, pins))
    })();
    match result {
        Ok(value) => Ok(value),
        Err(error) => match hal::HalCall::Exit.classify(unsafe { hal::hal_exit(component_id) }) {
            Ok(_) => Err(error),
            Err(cleanup) => Err(format!("{error}; hal_exit cleanup failed: {cleanup}")),
        },
    }
}
