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
        match self.kind() {
            HalFailureKind::SourceDeclared(errno) | HalFailureKind::KnownButUndeclared(errno) => {
                errno.name()
            }
            HalFailureKind::UnknownNegative => "UNKNOWN_ERRNO",
            HalFailureKind::InvalidZero => "INVALID_ZERO",
            HalFailureKind::UnexpectedPositive => "UNEXPECTED_POSITIVE",
        }
    }

    pub const fn c_label(self) -> &'static [u8] {
        match self.kind() {
            HalFailureKind::SourceDeclared(errno) | HalFailureKind::KnownButUndeclared(errno) => {
                errno.c_name()
            }
            HalFailureKind::UnknownNegative => b"UNKNOWN_ERRNO\0",
            HalFailureKind::InvalidZero => b"INVALID_ZERO\0",
            HalFailureKind::UnexpectedPositive => b"UNEXPECTED_POSITIVE\0",
        }
    }
}

impl fmt::Display for HalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind() {
            HalFailureKind::SourceDeclared(errno) => {
                write!(formatter, "{} ({})", errno.name(), self.raw)
            }
            HalFailureKind::KnownButUndeclared(errno) => write!(
                formatter,
                "{} ({}; not declared for {})",
                errno.name(),
                self.raw,
                self.call.name()
            ),
            HalFailureKind::UnknownNegative => {
                write!(formatter, "UNKNOWN_ERRNO ({})", self.raw)
            }
            HalFailureKind::InvalidZero => write!(
                formatter,
                "INVALID_ZERO (0; {} requires a nonzero failure or documented success)",
                self.call.name()
            ),
            HalFailureKind::UnexpectedPositive => write!(
                formatter,
                "UNEXPECTED_POSITIVE ({}; {} documents no positive status)",
                self.raw,
                self.call.name()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::collections::BTreeSet;
    use std::format;

    use super::*;

    #[test]
    fn every_bound_hal_call_has_one_unique_name_and_c_name() {
        assert_eq!(HAL_CALLS.len(), 10);
        assert_eq!(
            HAL_CALLS
                .iter()
                .map(|call| call.name())
                .collect::<BTreeSet<_>>()
                .len(),
            HAL_CALLS.len()
        );
        for call in HAL_CALLS {
            assert_eq!(call.c_name().last(), Some(&0));
            assert_eq!(
                &call.c_name()[..call.c_name().len() - 1],
                call.name().as_bytes()
            );
        }
    }

    #[test]
    fn every_source_declared_errno_is_exact_for_every_call() {
        for call in HAL_CALLS {
            for errno in HAL_KNOWN_ERRNOS {
                let error = call.classify(errno.raw()).unwrap_err();
                assert_eq!(error.call(), call);
                assert_eq!(error.raw(), errno.raw());
                assert_eq!(
                    error.source_declared(),
                    call.declares(errno),
                    "{} {}",
                    call.name(),
                    errno.name()
                );
                assert_eq!(error.safe_return_code(), errno.raw());
                assert_eq!(error.c_label().last(), Some(&0));
            }
        }
    }

    #[test]
    fn every_raw_result_class_fails_closed_without_losing_its_value() {
        assert_eq!(HalCall::Init.classify(1), Ok(1));
        assert_eq!(HalCall::Init.classify(c_int::MAX), Ok(c_int::MAX));
        for raw in [c_int::MIN, -2, -1, 0] {
            let error = HalCall::Init.classify(raw).unwrap_err();
            assert_eq!(error.raw(), raw);
            assert!(error.safe_return_code() < 0);
        }

        for call in HAL_CALLS.into_iter().filter(|call| *call != HalCall::Init) {
            assert_eq!(call.classify(0), Ok(0));
            for raw in [c_int::MIN, -22, -2, 1, c_int::MAX] {
                let error = call.classify(raw).unwrap_err();
                assert_eq!(error.raw(), raw);
                assert!(error.safe_return_code() < 0);
            }
        }
    }

    #[test]
    fn exact_operator_text_distinguishes_declared_unknown_and_contract_failures() {
        assert_eq!(
            format!(
                "{}",
                HalCall::Ready.classify(-(EINVAL as c_int)).unwrap_err()
            ),
            "EINVAL (-22)"
        );
        assert_eq!(
            format!(
                "{}",
                HalCall::Exit.classify(-(ENOMEM as c_int)).unwrap_err()
            ),
            "ENOMEM (-12; not declared for hal_exit)"
        );
        assert_eq!(
            format!("{}", HalCall::Ready.classify(-2).unwrap_err()),
            "UNKNOWN_ERRNO (-2)"
        );
        assert_eq!(
            format!("{}", HalCall::Init.classify(0).unwrap_err()),
            "INVALID_ZERO (0; hal_init requires a nonzero failure or documented success)"
        );
        assert_eq!(
            format!("{}", HalCall::Ready.classify(7).unwrap_err()),
            "UNEXPECTED_POSITIVE (7; hal_ready documents no positive status)"
        );
    }
}
