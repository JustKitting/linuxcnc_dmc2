//! Publish the retained calibration to AXIS-owned data parameters, not controls.
use dmc2_hal_sys as hal;
use dmc2ctl::calibration::{Calibration, Field, VALID_PARAMETER};
use std::ffi::CString;

pub(super) fn publish(calibration: Calibration) -> Result<(), String> {
    let name = CString::new(format!("dmc2-calibration-{}", std::process::id())).unwrap();
    let id = unsafe { hal::hal_init(name.as_ptr()) };
    if id < 0 {
        return Err(format!("CALIBRATION_DATA_UNAVAILABLE: cannot attach to AXIS data (HAL return {id}). Reopen the standard application before another measurement."));
    }
    let result = (|| {
        let valid = CString::new(format!("axisui.{VALID_PARAMETER}")).unwrap();
        checked(
            unsafe { hal::hal_param_bit_set(valid.as_ptr(), 0) },
            VALID_PARAMETER,
        )?;
        for field in Field::ALL {
            let parameter = CString::new(format!("axisui.{}", field.parameter())).unwrap();
            checked(
                unsafe { hal::hal_param_float_set(parameter.as_ptr(), calibration.value(field)) },
                field.parameter(),
            )?;
        }
        checked(
            unsafe { hal::hal_param_bit_set(valid.as_ptr(), 1) },
            VALID_PARAMETER,
        )
    })();
    let detached = unsafe { hal::hal_exit(id) };
    result.and_then(|_| checked(detached, "temporary calibration client detach"))
}

fn checked(result: i32, parameter: &str) -> Result<(), String> {
    if result == 0 {
        Ok(())
    } else {
        Err(format!("CALIBRATION_DATA_UNAVAILABLE: {parameter} was not accepted (HAL return {result}). Abort then Pendant Mode; reopen the matching application before retrying the measurement."))
    }
}
