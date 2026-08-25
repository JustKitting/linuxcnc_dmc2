use core::ffi::{c_char, c_int, c_long, c_void};
use core::mem::{align_of, size_of};

use crate::{
    hal_bit_t, hal_exit, hal_export_funct, hal_init, hal_malloc, hal_pin_bit_new,
    hal_pin_dir_t_HAL_IN, hal_pin_dir_t_HAL_IO, hal_pin_dir_t_HAL_OUT, hal_pin_float_new,
    hal_pin_s32_new, hal_pin_u32_new, hal_ready, hal_s32_t, hal_u32_t, msg_level_t,
    msg_level_t_RTAPI_MSG_ERR, real_t, rtapi_print_msg,
};

const _: unsafe extern "C" fn(*const c_char) -> c_int = hal_init;
const _: unsafe extern "C" fn(c_int) -> c_int = hal_exit;
const _: unsafe extern "C" fn(c_int) -> c_int = hal_ready;
const _: unsafe extern "C" fn(c_long) -> *mut c_void = hal_malloc;
const _: unsafe extern "C" fn(*const c_char, c_int, *mut *mut bool, c_int) -> c_int =
    hal_pin_bit_new;
const _: unsafe extern "C" fn(*const c_char, c_int, *mut *mut f64, c_int) -> c_int =
    hal_pin_float_new;
const _: unsafe extern "C" fn(*const c_char, c_int, *mut *mut i32, c_int) -> c_int =
    hal_pin_s32_new;
const _: unsafe extern "C" fn(*const c_char, c_int, *mut *mut u32, c_int) -> c_int =
    hal_pin_u32_new;
const _: unsafe extern "C" fn(
    *const c_char,
    Option<unsafe extern "C" fn(*mut c_void, c_long)>,
    *mut c_void,
    c_int,
    c_int,
    c_int,
) -> c_int = hal_export_funct;
const _: unsafe extern "C" fn(msg_level_t, *const c_char, ...) = rtapi_print_msg;

const _: [(); size_of::<bool>()] = [(); size_of::<hal_bit_t>()];
const _: [(); align_of::<bool>()] = [(); align_of::<hal_bit_t>()];
const _: [(); size_of::<f64>()] = [(); size_of::<real_t>()];
const _: [(); align_of::<f64>()] = [(); align_of::<real_t>()];
const _: [(); size_of::<i32>()] = [(); size_of::<hal_s32_t>()];
const _: [(); align_of::<i32>()] = [(); align_of::<hal_s32_t>()];
const _: [(); size_of::<u32>()] = [(); size_of::<hal_u32_t>()];
const _: [(); align_of::<u32>()] = [(); align_of::<hal_u32_t>()];

const _: [(); 16] = [(); hal_pin_dir_t_HAL_IN as usize];
const _: [(); 32] = [(); hal_pin_dir_t_HAL_OUT as usize];
const _: [(); 48] = [(); hal_pin_dir_t_HAL_IO as usize];
const _: [(); 1] = [(); msg_level_t_RTAPI_MSG_ERR as usize];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_bindings_match_the_audited_rust_abi() {
        assert_eq!(hal_pin_dir_t_HAL_IN, 16);
        assert_eq!(hal_pin_dir_t_HAL_OUT, 32);
        assert_eq!(hal_pin_dir_t_HAL_IO, 48);
        assert_eq!(msg_level_t_RTAPI_MSG_ERR, 1);
        assert_eq!(size_of::<hal_bit_t>(), size_of::<bool>());
        assert_eq!(size_of::<real_t>(), size_of::<f64>());
        assert_eq!(size_of::<hal_s32_t>(), size_of::<i32>());
        assert_eq!(size_of::<hal_u32_t>(), size_of::<u32>());
    }
}
