use std::ffi::{c_int, CString};
use std::{mem, ptr};

use dmc2_hal_sys as hal;

use super::pins::{
    HalPins, AXIS_OUTPUT_PINS, BIT_OUTPUT_PINS, MULTIPLIER_OUTPUT_PINS, PACKET_AGE_PIN,
    S32_OUTPUT_PINS, SNAPSHOT_GENERATION_PIN, U32_OUTPUT_PINS,
};

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
    (result == 0)
        .then_some(())
        .ok_or_else(|| format!("hal_pin_bit_new({suffix}) failed: {result}"))
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
    (result == 0)
        .then_some(())
        .ok_or_else(|| format!("hal_pin_s32_new({suffix}) failed: {result}"))
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
    (result == 0)
        .then_some(())
        .ok_or_else(|| format!("hal_pin_u32_new({suffix}) failed: {result}"))
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
    (result == 0)
        .then_some(())
        .ok_or_else(|| format!("hal_pin_float_new({suffix}) failed: {result}"))
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
    let component_id = unsafe { hal::hal_init(component_name.as_ptr()) };
    if component_id < 0 {
        return Err(format!("hal_init failed: {component_id}"));
    }

    let result = (|| {
        let pins = unsafe { hal::hal_malloc(mem::size_of::<HalPins>() as _) } as *mut HalPins;
        if pins.is_null() {
            return Err("hal_malloc for pin-pointer storage failed".to_owned());
        }
        unsafe {
            ptr::write(pins, HalPins::empty());
            register_pins(component, &mut *pins, component_id)?;
        }
        let ready = unsafe { hal::hal_ready(component_id) };
        if ready != 0 {
            return Err(format!("hal_ready failed: {ready}"));
        }
        Ok((component_id, pins))
    })();
    if result.is_err() {
        unsafe { hal::hal_exit(component_id) };
    }
    result
}
