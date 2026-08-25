use std::ffi::{c_int, CString};
use std::{mem, ptr};

use crate::application::nml::{required_cms_status, required_nml_error};
use crate::snapshot::{NativeSnapshot, SNAPSHOT_ABI_VERSION};
use dmc2_hal_sys as hal;

use super::pins::{
    HalPins, CLEAR_LATCHED_INPUT_PIN, CONNECTION_BIT_OUTPUT_PINS, DIAGNOSTIC_BIT_OUTPUT_PINS,
    DIAGNOSTIC_S32_OUTPUT_PINS, DIAGNOSTIC_U32_OUTPUT_PINS, MACHINE_BIT_OUTPUT_PINS,
    RUNTIME_U32_OUTPUT_PINS, SNAPSHOT_GENERATION_PIN,
};

unsafe fn new_bit_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::hal_bit_t,
    component_id: c_int,
    direction: hal::hal_pin_dir_t,
) -> Result<(), String> {
    let name = CString::new(format!("{component}.{suffix}"))
        .map_err(|_| "HAL pin name contained a NUL byte".to_owned())?;
    let result = unsafe { hal::hal_pin_bit_new(name.as_ptr(), direction, pointer, component_id) };
    if result == 0 {
        Ok(())
    } else {
        Err(format!("hal_pin_bit_new({suffix}) failed: {result}"))
    }
}

unsafe fn new_s32_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::hal_s32_t,
    component_id: c_int,
) -> Result<(), String> {
    let name = CString::new(format!("{component}.{suffix}"))
        .map_err(|_| "HAL pin name contained a NUL byte".to_owned())?;
    let result = unsafe {
        hal::hal_pin_s32_new(
            name.as_ptr(),
            hal::hal_pin_dir_t_HAL_OUT,
            pointer,
            component_id,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(format!("hal_pin_s32_new({suffix}) failed: {result}"))
    }
}

unsafe fn new_u32_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::hal_u32_t,
    component_id: c_int,
) -> Result<(), String> {
    let name = CString::new(format!("{component}.{suffix}"))
        .map_err(|_| "HAL pin name contained a NUL byte".to_owned())?;
    let result = unsafe {
        hal::hal_pin_u32_new(
            name.as_ptr(),
            hal::hal_pin_dir_t_HAL_OUT,
            pointer,
            component_id,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(format!("hal_pin_u32_new({suffix}) failed: {result}"))
    }
}

unsafe fn register_pins(
    component: &str,
    pins: &mut HalPins,
    component_id: c_int,
) -> Result<(), String> {
    let output = hal::hal_pin_dir_t_HAL_OUT;
    unsafe {
        new_u32_pin(
            component,
            SNAPSHOT_GENERATION_PIN,
            &mut pins.snapshot_generation,
            component_id,
        )?;
        for (suffix, pointer) in CONNECTION_BIT_OUTPUT_PINS
            .into_iter()
            .zip([&mut pins.connected, &mut pins.fault])
        {
            new_bit_pin(component, suffix, pointer, component_id, output)?;
        }
        for (suffix, pointer) in RUNTIME_U32_OUTPUT_PINS.into_iter().zip([
            &mut pins.task_heartbeat,
            &mut pins.publications,
            &mut pins.poll_errors,
        ]) {
            new_u32_pin(component, suffix, pointer, component_id)?;
        }
        for (suffix, pointer) in DIAGNOSTIC_BIT_OUTPUT_PINS.into_iter().zip([
            &mut pins.nml_error_known,
            &mut pins.cms_status_known,
            &mut pins.linuxcnc_error_active,
            &mut pins.linuxcnc_warning_active,
            &mut pins.unknown_code_active,
        ]) {
            new_bit_pin(component, suffix, pointer, component_id, output)?;
        }
        for (suffix, pointer) in DIAGNOSTIC_U32_OUTPUT_PINS.into_iter().zip([
            &mut pins.active_error_mask_low,
            &mut pins.active_error_mask_high,
            &mut pins.active_warning_mask_low,
            &mut pins.active_warning_mask_high,
            &mut pins.latched_error_mask_low,
            &mut pins.latched_error_mask_high,
            &mut pins.latched_warning_mask_low,
            &mut pins.latched_warning_mask_high,
            &mut pins.unknown_domain_mask_low,
            &mut pins.unknown_domain_mask_high,
            &mut pins.diagnostic_count,
            &mut pins.unknown_code_count,
            &mut pins.diagnostic_transitions,
            &mut pins.latest_code_low,
            &mut pins.latest_code_high,
            &mut pins.snapshot_abi_version,
            &mut pins.snapshot_struct_size,
        ]) {
            new_u32_pin(component, suffix, pointer, component_id)?;
        }
        for (suffix, pointer) in DIAGNOSTIC_S32_OUTPUT_PINS.into_iter().zip([
            &mut pins.nml_error_code,
            &mut pins.cms_status_code,
            &mut pins.latest_code_domain,
            &mut pins.latest_severity,
            &mut pins.latest_action,
        ]) {
            new_s32_pin(component, suffix, pointer, component_id)?;
        }
        new_bit_pin(
            component,
            CLEAR_LATCHED_INPUT_PIN,
            &mut pins.clear_latched,
            component_id,
            hal::hal_pin_dir_t_HAL_IN,
        )?;
        for (suffix, pointer) in MACHINE_BIT_OUTPUT_PINS.into_iter().zip([
            &mut pins.machine_on,
            &mut pins.estopped,
            &mut pins.manual_mode,
            &mut pins.joint_mode,
            &mut pins.teleop_mode,
            &mut pins.interp_idle,
        ]) {
            new_bit_pin(component, suffix, pointer, component_id, output)?;
        }
        for index in 0..3 {
            new_bit_pin(
                component,
                &format!("joint-{index}-homed"),
                &mut pins.homed[index],
                component_id,
                output,
            )?;
            new_bit_pin(
                component,
                &format!("joint-{index}-homing"),
                &mut pins.homing[index],
                component_id,
                output,
            )?;
            new_bit_pin(
                component,
                &format!("axis-{index}-stopped"),
                &mut pins.axis_stopped[index],
                component_id,
                output,
            )?;
        }
    }
    Ok(())
}

unsafe fn publish_initial_safe(pins: &HalPins) {
    unsafe {
        ptr::write_volatile(pins.snapshot_generation, 1);
        ptr::write_volatile(pins.connected, false);
        ptr::write_volatile(pins.fault, true);
        ptr::write_volatile(pins.task_heartbeat, 0);
        ptr::write_volatile(pins.publications, 0);
        ptr::write_volatile(pins.poll_errors, 0);
        let initial_nml_error = required_nml_error("NML_INVALID_CONFIGURATION");
        ptr::write_volatile(pins.nml_error_code, initial_nml_error);
        ptr::write_volatile(pins.nml_error_known, true);
        ptr::write_volatile(
            pins.cms_status_code,
            required_cms_status("CMS_STATUS_NOT_SET"),
        );
        ptr::write_volatile(pins.cms_status_known, true);
        ptr::write_volatile(pins.linuxcnc_error_active, true);
        ptr::write_volatile(pins.linuxcnc_warning_active, false);
        ptr::write_volatile(pins.unknown_code_active, false);
        ptr::write_volatile(pins.active_error_mask_low, 0);
        ptr::write_volatile(pins.active_error_mask_high, 0);
        ptr::write_volatile(pins.active_warning_mask_low, 0);
        ptr::write_volatile(pins.active_warning_mask_high, 0);
        ptr::write_volatile(pins.latched_error_mask_low, 0);
        ptr::write_volatile(pins.latched_error_mask_high, 0);
        ptr::write_volatile(pins.latched_warning_mask_low, 0);
        ptr::write_volatile(pins.latched_warning_mask_high, 0);
        ptr::write_volatile(pins.unknown_domain_mask_low, 0);
        ptr::write_volatile(pins.unknown_domain_mask_high, 0);
        ptr::write_volatile(pins.diagnostic_count, 0);
        ptr::write_volatile(pins.unknown_code_count, 0);
        ptr::write_volatile(pins.diagnostic_transitions, 0);
        ptr::write_volatile(pins.latest_code_domain, -1);
        ptr::write_volatile(pins.latest_code_low, 0);
        ptr::write_volatile(pins.latest_code_high, 0);
        ptr::write_volatile(pins.latest_severity, 0);
        ptr::write_volatile(pins.latest_action, 0);
        ptr::write_volatile(pins.clear_latched, false);
        ptr::write_volatile(pins.snapshot_abi_version, SNAPSHOT_ABI_VERSION);
        ptr::write_volatile(
            pins.snapshot_struct_size,
            mem::size_of::<NativeSnapshot>() as u32,
        );
        ptr::write_volatile(pins.machine_on, false);
        ptr::write_volatile(pins.estopped, true);
        ptr::write_volatile(pins.manual_mode, false);
        ptr::write_volatile(pins.joint_mode, false);
        ptr::write_volatile(pins.teleop_mode, false);
        ptr::write_volatile(pins.interp_idle, false);
        for index in 0..3 {
            ptr::write_volatile(pins.homed[index], false);
            ptr::write_volatile(pins.homing[index], false);
            ptr::write_volatile(pins.axis_stopped[index], true);
        }
        ptr::write_volatile(pins.snapshot_generation, 0);
    }
}

pub(super) unsafe fn create_hal(component: &str) -> Result<(c_int, *mut HalPins), String> {
    let component_name = CString::new(component)
        .map_err(|_| "HAL component name contained a NUL byte".to_owned())?;
    let component_id = unsafe { hal::hal_init(component_name.as_ptr()) };
    if component_id <= 0 {
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
            publish_initial_safe(&*pins);
        }
        let ready = unsafe { hal::hal_ready(component_id) };
        if ready != 0 {
            return Err(format!("hal_ready failed: {ready}"));
        }
        Ok((component_id, pins))
    })();

    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let cleanup = unsafe { hal::hal_exit(component_id) };
            if cleanup == 0 {
                Err(error)
            } else {
                Err(format!("{error}; hal_exit cleanup failed: {cleanup}"))
            }
        }
    }
}
