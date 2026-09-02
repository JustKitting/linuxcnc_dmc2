use std::ffi::{c_int, CStr};
use std::fmt;
use std::mem::MaybeUninit;

use super::ffi;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::application) enum Error {
    OperatingSystem {
        operation: &'static str,
        errno: c_int,
    },
    Contract {
        operation: &'static str,
        result: isize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OperatingSystem { operation, errno } => write!(
                formatter,
                "POSIX_OPERATING_SYSTEM_FAILURE (operation={operation}, errno={errno}, meaning={:?}): the named serial transport operation failed; action: correct the reported OS condition for that operation before reconnecting",
                std::io::Error::from_raw_os_error(*errno)
            ),
            Self::Contract { operation, result } => {
                write!(
                    formatter,
                    "POSIX_RESULT_CONTRACT_VIOLATION (operation={operation}, raw={result}): the named POSIX operation returned a value outside its verified result contract; action: retain the operation and raw result, stop the bridge, and verify the running libc/kernel ABI"
                )
            }
        }
    }
}

impl Error {
    pub(in crate::application) const fn operating_system_error(self) -> Option<c_int> {
        match self {
            Self::OperatingSystem { errno, .. } => Some(errno),
            Self::Contract { .. } => None,
        }
    }

    pub(in crate::application) const fn contract_result(self) -> Option<i64> {
        match self {
            Self::OperatingSystem { .. } => None,
            Self::Contract { result, .. } => Some(result as i64),
        }
    }

    const fn operating_system(operation: &'static str, errno: c_int) -> Self {
        Self::OperatingSystem { operation, errno }
    }
}

fn last_errno() -> c_int {
    let errno = unsafe { *ffi::__errno_location() };
    if errno > 0 {
        errno
    } else {
        ffi::EIO as c_int
    }
}

fn invalid_argument(operation: &'static str) -> Error {
    Error::operating_system(operation, ffi::EINVAL as c_int)
}

fn close_after_failure(descriptor: c_int, operation_error: Error) -> Error {
    close(descriptor).err().unwrap_or(operation_error)
}

fn baud_speed(baud: u32) -> Option<ffi::speed_t> {
    match baud {
        115_200 => Some(ffi::B115200 as ffi::speed_t),
        _ => None,
    }
}

fn configure(descriptor: c_int, speed: ffi::speed_t) -> Result<(), Error> {
    let mut options = MaybeUninit::<ffi::termios>::uninit();
    if unsafe { ffi::tcgetattr(descriptor, options.as_mut_ptr()) } != 0 {
        return Err(Error::operating_system("tcgetattr", last_errno()));
    }
    let mut options = unsafe { options.assume_init() };
    unsafe { ffi::cfmakeraw(&mut options) };
    options.c_cflag |= (ffi::CLOCAL | ffi::CREAD) as ffi::tcflag_t;
    options.c_cflag &= !((ffi::CSTOPB | ffi::CRTSCTS) as ffi::tcflag_t);
    options.c_cflag &= !(ffi::CSIZE as ffi::tcflag_t);
    options.c_cflag |= ffi::CS8 as ffi::tcflag_t;
    options.c_cc[ffi::VMIN as usize] = 0;
    options.c_cc[ffi::VTIME as usize] = 0;

    if unsafe { ffi::cfsetispeed(&mut options, speed) } != 0 {
        return Err(Error::operating_system("cfsetispeed", last_errno()));
    }
    if unsafe { ffi::cfsetospeed(&mut options, speed) } != 0 {
        return Err(Error::operating_system("cfsetospeed", last_errno()));
    }
    if unsafe { ffi::tcsetattr(descriptor, ffi::TCSANOW as c_int, &options) } != 0 {
        return Err(Error::operating_system("tcsetattr", last_errno()));
    }
    if unsafe { ffi::tcflush(descriptor, ffi::TCIFLUSH as c_int) } != 0 {
        return Err(Error::operating_system("tcflush", last_errno()));
    }
    Ok(())
}

pub(super) fn open(path: &CStr, baud: u32) -> Result<c_int, Error> {
    let speed = baud_speed(baud).ok_or_else(|| invalid_argument("validate-supported-baud"))?;
    if path.to_bytes().is_empty() {
        return Err(invalid_argument("validate-device-path"));
    }
    let flags = ffi::O_RDWR | ffi::O_NOCTTY | ffi::O_NONBLOCK | ffi::O_CLOEXEC;
    let descriptor = unsafe { ffi::open(path.as_ptr(), flags as c_int) };
    if descriptor < 0 {
        return Err(Error::operating_system("open", last_errno()));
    }
    if let Err(error) = configure(descriptor, speed) {
        return Err(close_after_failure(descriptor, error));
    }
    Ok(descriptor)
}

fn validate_read(descriptor: c_int, capacity: usize) -> Result<(), Error> {
    if descriptor < 0 || capacity == 0 || capacity > c_int::MAX as usize {
        Err(invalid_argument("validate-read-arguments"))
    } else {
        Ok(())
    }
}

pub(super) fn read(descriptor: c_int, buffer: &mut [u8]) -> Result<usize, Error> {
    validate_read(descriptor, buffer.len())?;
    let result = unsafe {
        ffi::read(
            descriptor,
            buffer.as_mut_ptr().cast::<std::ffi::c_void>(),
            buffer.len(),
        )
    };
    if result < 0 {
        let errno = last_errno();
        if errno == ffi::EAGAIN as c_int
            || errno == ffi::EWOULDBLOCK as c_int
            || errno == ffi::EINTR as c_int
        {
            return Ok(0);
        }
        return Err(Error::operating_system("read", errno));
    }
    let count = result as usize;
    if count > buffer.len() {
        Err(Error::Contract {
            operation: "read",
            result,
        })
    } else {
        Ok(count)
    }
}

pub(super) fn close(descriptor: c_int) -> Result<(), Error> {
    if descriptor < 0 {
        return Err(invalid_argument("validate-close-descriptor"));
    }
    if unsafe { ffi::close(descriptor) } == 0 {
        Ok(())
    } else {
        Err(Error::operating_system("close", last_errno()))
    }
}
