use std::ffi::{c_char, c_int, CStr};
use std::fs::File;
use std::os::fd::{FromRawFd, RawFd};

use crate::failure::{Failure, FailureCode, Result};

const O_RDWR: c_int = 0x0002;
const O_NOCTTY: c_int = 0x0100;
const O_CLOEXEC: c_int = 0x80000;

unsafe extern "C" {
    fn posix_openpt(flags: c_int) -> c_int;
    fn grantpt(fd: c_int) -> c_int;
    fn unlockpt(fd: c_int) -> c_int;
    fn ptsname_r(fd: c_int, buffer: *mut c_char, length: usize) -> c_int;
    fn open(path: *const c_char, flags: c_int, ...) -> c_int;
    fn close(fd: c_int) -> c_int;
}

pub(crate) struct PseudoTerminal {
    master: Option<File>,
    _held_slave: File,
    slave_path: String,
}

impl PseudoTerminal {
    pub(crate) fn open() -> Result<Self> {
        let master = unsafe { posix_openpt(O_RDWR | O_NOCTTY | O_CLOEXEC) };
        if master < 0 {
            return Err(last_os_failure("posix_openpt"));
        }
        if unsafe { grantpt(master) } != 0 {
            unsafe { close(master) };
            return Err(last_os_failure("grantpt"));
        }
        if unsafe { unlockpt(master) } != 0 {
            unsafe { close(master) };
            return Err(last_os_failure("unlockpt"));
        }

        let mut path = [0 as c_char; 256];
        let result = unsafe { ptsname_r(master, path.as_mut_ptr(), path.len()) };
        if result != 0 {
            unsafe { close(master) };
            return Err(Failure::new(
                FailureCode::PendantTransport,
                format!("operation=ptsname_r; result={result}"),
            ));
        }
        let slave_path = unsafe { CStr::from_ptr(path.as_ptr()) }
            .to_str()
            .map_err(|error| {
                unsafe { close(master) };
                Failure::new(
                    FailureCode::PendantTransport,
                    format!("PTY slave path was not UTF-8: {error}"),
                )
            })?
            .to_owned();
        let slave = unsafe { open(path.as_ptr(), O_RDWR | O_NOCTTY | O_CLOEXEC) };
        if slave < 0 {
            unsafe { close(master) };
            return Err(last_os_failure("open PTY slave"));
        }

        Ok(Self {
            master: Some(unsafe { File::from_raw_fd(master as RawFd) }),
            _held_slave: unsafe { File::from_raw_fd(slave as RawFd) },
            slave_path,
        })
    }

    pub(crate) fn slave_path(&self) -> &str {
        &self.slave_path
    }

    pub(crate) fn take_master(&mut self) -> Result<File> {
        self.master.take().ok_or_else(|| {
            Failure::new(
                FailureCode::PendantTransport,
                "PTY master was already assigned to a packet stream",
            )
        })
    }
}

fn last_os_failure(operation: &'static str) -> Failure {
    Failure::io(
        FailureCode::PendantTransport,
        operation,
        std::io::Error::last_os_error(),
    )
}
