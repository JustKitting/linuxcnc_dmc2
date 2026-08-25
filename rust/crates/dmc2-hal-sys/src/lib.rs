#![no_std]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

// This file contains no handwritten LinuxCNC ABI declarations. Cargo invokes
// bindgen against this computer's installed /usr/include/linuxcnc/hal.h and
// refuses any LinuxCNC version other than the audited 2.9.10 installation.
include!(concat!(env!("OUT_DIR"), "/hal_bindings.rs"));
