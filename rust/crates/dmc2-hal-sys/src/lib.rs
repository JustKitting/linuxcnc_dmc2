#![no_std]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

// This file contains no handwritten LinuxCNC ABI declarations. Cargo invokes
// bindgen against this computer's installed /usr/include/linuxcnc/hal.h and
// refuses any LinuxCNC version other than the audited 2.9.10 installation.
include!(concat!(env!("OUT_DIR"), "/hal_bindings.rs"));

mod abi;
mod catalog;
mod pin;
pub mod probe_stream;
mod registration_error;
mod return_code;

pub use pin::{register_numbered_pin, register_pin, HalPinDirection, HalPinKind, HalPinValue};
pub use registration_error::{HalNameKind, HalRegistrationError, HalRegistrationFailure};
pub use return_code::{
    HalCall, HalError, HalFailureKind, HalKnownErrno, HAL_CALLS, HAL_KNOWN_ERRNOS,
};
