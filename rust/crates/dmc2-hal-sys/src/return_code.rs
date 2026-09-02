//! Exhaustive classification for every numeric LinuxCNC HAL call we bind.

use core::ffi::c_int;
use core::fmt;

use crate::{EINVAL, ENOMEM, EPERM};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HalCall {
    Init,
    Exit,
    Ready,
    PinBitNew,
    PinFloatNew,
    PinS32New,
    PinU32New,
    ParamFloatNew,
    ParamU32New,
    ExportFunct,
}

pub const HAL_CALLS: [HalCall; 10] = [
    HalCall::Init,
    HalCall::Exit,
    HalCall::Ready,
    HalCall::PinBitNew,
    HalCall::PinFloatNew,
    HalCall::PinS32New,
    HalCall::PinU32New,
    HalCall::ParamFloatNew,
    HalCall::ParamU32New,
    HalCall::ExportFunct,
];

impl HalCall {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Init => "hal_init",
            Self::Exit => "hal_exit",
            Self::Ready => "hal_ready",
            Self::PinBitNew => "hal_pin_bit_new",
            Self::PinFloatNew => "hal_pin_float_new",
            Self::PinS32New => "hal_pin_s32_new",
            Self::PinU32New => "hal_pin_u32_new",
            Self::ParamFloatNew => "hal_param_float_new",
            Self::ParamU32New => "hal_param_u32_new",
            Self::ExportFunct => "hal_export_funct",
        }
    }

    pub const fn c_name(self) -> &'static [u8] {
        match self {
            Self::Init => b"hal_init\0",
            Self::Exit => b"hal_exit\0",
            Self::Ready => b"hal_ready\0",
            Self::PinBitNew => b"hal_pin_bit_new\0",
            Self::PinFloatNew => b"hal_pin_float_new\0",
            Self::PinS32New => b"hal_pin_s32_new\0",
            Self::PinU32New => b"hal_pin_u32_new\0",
            Self::ParamFloatNew => b"hal_param_float_new\0",
            Self::ParamU32New => b"hal_param_u32_new\0",
            Self::ExportFunct => b"hal_export_funct\0",
        }
    }

    pub const fn classify(self, raw: c_int) -> Result<c_int, HalError> {
        let success = match self {
            Self::Init => raw > 0,
            _ => raw == 0,
        };
        if success {
            Ok(raw)
        } else {
            Err(HalError { call: self, raw })
        }
    }

    const fn declares(self, errno: HalKnownErrno) -> bool {
        match self {
            Self::Init => matches!(
                errno,
                HalKnownErrno::InvalidArgument | HalKnownErrno::OutOfMemory
            ),
            Self::Exit | Self::Ready => matches!(errno, HalKnownErrno::InvalidArgument),
            Self::PinBitNew
            | Self::PinFloatNew
            | Self::PinS32New
            | Self::PinU32New
            | Self::ParamFloatNew
            | Self::ParamU32New
            | Self::ExportFunct => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HalKnownErrno {
    PermissionDenied,
    OutOfMemory,
    InvalidArgument,
}

pub const HAL_KNOWN_ERRNOS: [HalKnownErrno; 3] = [
    HalKnownErrno::PermissionDenied,
    HalKnownErrno::OutOfMemory,
    HalKnownErrno::InvalidArgument,
];

impl HalKnownErrno {
    pub const fn name(self) -> &'static str {
        match self {
            Self::PermissionDenied => "EPERM",
            Self::OutOfMemory => "ENOMEM",
            Self::InvalidArgument => "EINVAL",
        }
    }

    pub const fn c_name(self) -> &'static [u8] {
        match self {
            Self::PermissionDenied => b"EPERM\0",
            Self::OutOfMemory => b"ENOMEM\0",
            Self::InvalidArgument => b"EINVAL\0",
        }
    }

    pub const fn raw(self) -> c_int {
        match self {
            Self::PermissionDenied => -(EPERM as c_int),
            Self::OutOfMemory => -(ENOMEM as c_int),
            Self::InvalidArgument => -(EINVAL as c_int),
        }
    }

    pub const fn summary(self) -> &'static str {
        match self {
            Self::PermissionDenied => {
                "LinuxCNC HAL denied the requested operation because the caller lacks permission"
            }
            Self::OutOfMemory => {
                "LinuxCNC HAL could not allocate the shared-memory resources required by the operation"
            }
            Self::InvalidArgument => {
                "LinuxCNC HAL rejected an argument, component identifier, name, direction, or lifecycle state"
            }
        }
    }

    pub const fn action(self) -> &'static str {
        match self {
            Self::PermissionDenied => {
                "verify LinuxCNC/HAL ownership and process privileges before retrying the named call"
            }
            Self::OutOfMemory => {
                "stop duplicate HAL owners and restore sufficient HAL shared memory before restarting"
            }
            Self::InvalidArgument => {
                "inspect the retained call and raw value, then verify its component id, name, direction, and lifecycle ordering"
            }
        }
    }

    const fn from_raw(raw: c_int) -> Option<Self> {
        if raw == Self::PermissionDenied.raw() {
            Some(Self::PermissionDenied)
        } else if raw == Self::OutOfMemory.raw() {
            Some(Self::OutOfMemory)
        } else if raw == Self::InvalidArgument.raw() {
            Some(Self::InvalidArgument)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HalFailureKind {
    SourceDeclared(HalKnownErrno),
    KnownButUndeclared(HalKnownErrno),
    UnknownNegative,
    InvalidZero,
    UnexpectedPositive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HalError {
    call: HalCall,
    raw: c_int,
}

impl HalError {
    pub const fn call(self) -> HalCall {
        self.call
    }

    pub const fn raw(self) -> c_int {
        self.raw
    }

    pub const fn kind(self) -> HalFailureKind {
        if let Some(errno) = HalKnownErrno::from_raw(self.raw) {
            if self.call.declares(errno) {
                HalFailureKind::SourceDeclared(errno)
            } else {
                HalFailureKind::KnownButUndeclared(errno)
            }
        } else if self.raw < 0 {
            HalFailureKind::UnknownNegative
        } else if self.raw == 0 {
            HalFailureKind::InvalidZero
        } else {
            HalFailureKind::UnexpectedPositive
        }
    }

    pub const fn source_declared(self) -> bool {
        matches!(self.kind(), HalFailureKind::SourceDeclared(_))
    }

    pub const fn safe_return_code(self) -> c_int {
        if self.raw < 0 {
            self.raw
        } else {
            HalKnownErrno::InvalidArgument.raw()
        }
    }

    pub const fn label(self) -> &'static str {
        self.identity()
    }

    /// Stable operator-visible identity for this exact return-value class.
    pub const fn identity(self) -> &'static str {
        match self.kind() {
            HalFailureKind::SourceDeclared(errno) | HalFailureKind::KnownButUndeclared(errno) => {
                errno.name()
            }
            HalFailureKind::UnknownNegative => "HAL_UNKNOWN_NEGATIVE_RETURN",
            HalFailureKind::InvalidZero => "HAL_INVALID_ZERO_RETURN",
            HalFailureKind::UnexpectedPositive => "HAL_UNEXPECTED_POSITIVE_RETURN",
        }
    }

    /// Plain-language cause, available without a separate errno table.
    pub const fn summary(self) -> &'static str {
        match self.kind() {
            HalFailureKind::SourceDeclared(errno) => errno.summary(),
            HalFailureKind::KnownButUndeclared(_) => {
                "LinuxCNC HAL returned a known errno that the pinned 2.9.10 source does not declare for this call"
            }
            HalFailureKind::UnknownNegative => {
                "LinuxCNC HAL returned a negative value absent from the verified HAL errno catalog"
            }
            HalFailureKind::InvalidZero => {
                "LinuxCNC HAL returned zero even though this call requires a positive success identifier"
            }
            HalFailureKind::UnexpectedPositive => {
                "LinuxCNC HAL returned a positive value even though this call documents only zero as success"
            }
        }
    }

    /// Concrete next step paired with every numeric HAL failure.
    pub const fn action(self) -> &'static str {
        match self.kind() {
            HalFailureKind::SourceDeclared(errno) => errno.action(),
            HalFailureKind::KnownButUndeclared(_) | HalFailureKind::UnknownNegative => {
                "retain the named call and raw value, stop launch, and verify the running HAL ABI against pinned LinuxCNC 2.9.10"
            }
            HalFailureKind::InvalidZero | HalFailureKind::UnexpectedPositive => {
                "retain the named call and raw value, stop launch, and verify the HAL ABI and component lifecycle contract"
            }
        }
    }

    pub const fn c_label(self) -> &'static [u8] {
        match self.kind() {
            HalFailureKind::SourceDeclared(errno) | HalFailureKind::KnownButUndeclared(errno) => {
                errno.c_name()
            }
            HalFailureKind::UnknownNegative => b"HAL_UNKNOWN_NEGATIVE_RETURN\0",
            HalFailureKind::InvalidZero => b"HAL_INVALID_ZERO_RETURN\0",
            HalFailureKind::UnexpectedPositive => b"HAL_UNEXPECTED_POSITIVE_RETURN\0",
        }
    }

    pub const fn c_summary(self) -> &'static [u8] {
        match self.kind() {
            HalFailureKind::SourceDeclared(HalKnownErrno::PermissionDenied) => b"LinuxCNC HAL denied the requested operation because the caller lacks permission\0",
            HalFailureKind::SourceDeclared(HalKnownErrno::OutOfMemory) => b"LinuxCNC HAL could not allocate the shared-memory resources required by the operation\0",
            HalFailureKind::SourceDeclared(HalKnownErrno::InvalidArgument) => b"LinuxCNC HAL rejected an argument, component identifier, name, direction, or lifecycle state\0",
            HalFailureKind::KnownButUndeclared(_) => b"LinuxCNC HAL returned a known errno that the pinned 2.9.10 source does not declare for this call\0",
            HalFailureKind::UnknownNegative => b"LinuxCNC HAL returned a negative value absent from the verified HAL errno catalog\0",
            HalFailureKind::InvalidZero => b"LinuxCNC HAL returned zero even though this call requires a positive success identifier\0",
            HalFailureKind::UnexpectedPositive => b"LinuxCNC HAL returned a positive value even though this call documents only zero as success\0",
        }
    }

    pub const fn c_action(self) -> &'static [u8] {
        match self.kind() {
            HalFailureKind::SourceDeclared(HalKnownErrno::PermissionDenied) => b"verify LinuxCNC/HAL ownership and process privileges before retrying the named call\0",
            HalFailureKind::SourceDeclared(HalKnownErrno::OutOfMemory) => b"stop duplicate HAL owners and restore sufficient HAL shared memory before restarting\0",
            HalFailureKind::SourceDeclared(HalKnownErrno::InvalidArgument) => b"inspect the retained call and raw value, then verify its component id, name, direction, and lifecycle ordering\0",
            HalFailureKind::KnownButUndeclared(_) | HalFailureKind::UnknownNegative => b"retain the named call and raw value, stop launch, and verify the running HAL ABI against pinned LinuxCNC 2.9.10\0",
            HalFailureKind::InvalidZero | HalFailureKind::UnexpectedPositive => b"retain the named call and raw value, stop launch, and verify the HAL ABI and component lifecycle contract\0",
        }
    }
}

impl fmt::Display for HalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} (raw={}, call={}): {}; action: {}",
            self.identity(),
            self.raw,
            self.call.name(),
            self.summary(),
            self.action()
        )
    }
}
