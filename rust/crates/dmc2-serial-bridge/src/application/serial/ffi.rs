#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use std::ffi::{c_char, c_int, c_void};

include!(concat!(env!("OUT_DIR"), "/serial_posix_bindings.rs"));

const _: unsafe extern "C" fn() -> *mut c_int = __errno_location;
const _: unsafe extern "C" fn(*const c_char, c_int, ...) -> c_int = open;
const _: unsafe extern "C" fn(c_int) -> c_int = close;
const _: unsafe extern "C" fn(c_int, *mut c_void, usize) -> isize = read;
const _: unsafe extern "C" fn(c_int, *mut termios) -> c_int = tcgetattr;
const _: unsafe extern "C" fn(*mut termios) = cfmakeraw;
const _: unsafe extern "C" fn(*mut termios, speed_t) -> c_int = cfsetispeed;
const _: unsafe extern "C" fn(*mut termios, speed_t) -> c_int = cfsetospeed;
const _: unsafe extern "C" fn(c_int, c_int, *const termios) -> c_int = tcsetattr;
const _: unsafe extern "C" fn(c_int, c_int) -> c_int = tcflush;
