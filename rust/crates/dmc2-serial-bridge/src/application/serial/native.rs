#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use std::ffi::{c_char, c_int, c_uint};

include!(concat!(env!("OUT_DIR"), "/serial_shim_bindings.rs"));

const _: unsafe extern "C" fn(*const c_char, c_uint) -> c_int = dmc2_serial_open;
const _: unsafe extern "C" fn(c_int, *mut u8, usize) -> c_int = dmc2_serial_read;
const _: unsafe extern "C" fn(c_int) -> c_int = dmc2_serial_close;
