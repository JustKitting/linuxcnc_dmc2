//! One typed definition of LinuxCNC HAL pin kinds and directions.

use core::ffi::{c_char, c_int};

use crate::{
    hal_bit_t, hal_pin_bit_new, hal_pin_dir_t, hal_pin_dir_t_HAL_IN, hal_pin_dir_t_HAL_IO,
    hal_pin_dir_t_HAL_OUT, hal_pin_float_new, hal_pin_s32_new, hal_pin_u32_new, hal_s32_t,
    hal_u32_t, real_t, HalCall, HalError,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HalPinKind {
    Bit,
    S32,
    U32,
    Float,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HalPinDirection {
    In,
    Out,
    Io,
}

impl HalPinDirection {
    pub const fn raw(self) -> hal_pin_dir_t {
        match self {
            Self::In => hal_pin_dir_t_HAL_IN,
            Self::Out => hal_pin_dir_t_HAL_OUT,
            Self::Io => hal_pin_dir_t_HAL_IO,
        }
    }
}

mod sealed {
    pub trait Sealed {}

    impl Sealed for bool {}
    impl Sealed for i32 {}
    impl Sealed for u32 {}
    impl Sealed for f64 {}
}

/// A primitive type that LinuxCNC 2.9.10 accepts as HAL pin storage.
///
/// This trait is sealed so callers cannot associate an arbitrary Rust type
/// with one of LinuxCNC's C registration functions.
pub trait HalPinValue: sealed::Sealed + Copy {
    const KIND: HalPinKind;
    const CALL: HalCall;
    const ZERO: Self;

    unsafe fn register(
        name: *const c_char,
        direction: hal_pin_dir_t,
        pointer: *mut *mut Self,
        component_id: c_int,
    ) -> c_int;
}

impl HalPinValue for hal_bit_t {
    const KIND: HalPinKind = HalPinKind::Bit;
    const CALL: HalCall = HalCall::PinBitNew;
    const ZERO: Self = false;

    unsafe fn register(
        name: *const c_char,
        direction: hal_pin_dir_t,
        pointer: *mut *mut Self,
        component_id: c_int,
    ) -> c_int {
        unsafe { hal_pin_bit_new(name, direction, pointer, component_id) }
    }
}

impl HalPinValue for hal_s32_t {
    const KIND: HalPinKind = HalPinKind::S32;
    const CALL: HalCall = HalCall::PinS32New;
    const ZERO: Self = 0;

    unsafe fn register(
        name: *const c_char,
        direction: hal_pin_dir_t,
        pointer: *mut *mut Self,
        component_id: c_int,
    ) -> c_int {
        unsafe { hal_pin_s32_new(name, direction, pointer, component_id) }
    }
}

impl HalPinValue for hal_u32_t {
    const KIND: HalPinKind = HalPinKind::U32;
    const CALL: HalCall = HalCall::PinU32New;
    const ZERO: Self = 0;

    unsafe fn register(
        name: *const c_char,
        direction: hal_pin_dir_t,
        pointer: *mut *mut Self,
        component_id: c_int,
    ) -> c_int {
        unsafe { hal_pin_u32_new(name, direction, pointer, component_id) }
    }
}

impl HalPinValue for real_t {
    const KIND: HalPinKind = HalPinKind::Float;
    const CALL: HalCall = HalCall::PinFloatNew;
    const ZERO: Self = 0.0;

    unsafe fn register(
        name: *const c_char,
        direction: hal_pin_dir_t,
        pointer: *mut *mut Self,
        component_id: c_int,
    ) -> c_int {
        unsafe { hal_pin_float_new(name, direction, pointer, component_id) }
    }
}

/// Invoke the only LinuxCNC registration function valid for `T` and classify
/// its return code with the matching typed `HalCall`.
pub unsafe fn register_pin<T: HalPinValue>(
    name: *const c_char,
    direction: HalPinDirection,
    pointer: *mut *mut T,
    component_id: c_int,
) -> Result<(), HalError> {
    T::CALL
        .classify(unsafe { T::register(name, direction.raw(), pointer, component_id) })
        .map(|_| ())
}

/// Register a pin whose stable HAL name ends in a catalog-owned numeric code.
///
/// Realtime components cannot allocate or format strings. This bounded helper
/// constructs the decimal suffix on the stack and retains the same typed call
/// classification as [`register_pin`].
pub unsafe fn register_numbered_pin<T: HalPinValue>(
    prefix: &[u8],
    value: u32,
    direction: HalPinDirection,
    pointer: *mut *mut T,
    component_id: c_int,
) -> Result<(), HalError> {
    let mut name = [0_u8; crate::HAL_NAME_LEN as usize + 1];
    let mut reversed = [0_u8; 10];
    let mut remaining = value;
    let mut digits = 0;
    loop {
        reversed[digits] = b'0' + (remaining % 10) as u8;
        digits += 1;
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }
    let end = prefix.len() + digits;
    if end > crate::HAL_NAME_LEN as usize {
        return T::CALL
            .classify(crate::HalKnownErrno::InvalidArgument.raw())
            .map(|_| ());
    }
    name[..prefix.len()].copy_from_slice(prefix);
    for index in 0..digits {
        name[prefix.len() + index] = reversed[digits - index - 1];
    }
    unsafe { register_pin(name.as_ptr().cast(), direction, pointer, component_id) }
}
