//! Native LinuxCNC realtime component for the DMC2 pendant controller.
//!
//! The crate root is intentionally only the ABI facade. Component lifecycle,
//! HAL transport, and deterministic test infrastructure live below it.

// Release builds are the installable realtime module and are deliberately
// no_std. Debug/test builds use std only so Cargo's test harness can link.
#![cfg_attr(not(debug_assertions), no_std)]

mod component;

pub use component::{rtapi_app_exit, rtapi_app_main};
