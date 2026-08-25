use std::ffi::{c_int, CString};
use std::fmt;

use super::native;

pub(in crate::application) struct SerialPort(c_int);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::application) enum SerialError {
    OperatingSystem(c_int),
    NativeContract {
        operation: &'static str,
        result: c_int,
    },
}

impl SerialError {
    fn from_failure(operation: &'static str, result: c_int) -> Self {
        match result.checked_neg().filter(|errno| *errno > 0) {
            Some(errno) => Self::OperatingSystem(errno),
            None => Self::NativeContract { operation, result },
        }
    }
}

impl fmt::Display for SerialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OperatingSystem(errno) => write!(
                formatter,
                "{} (errno {errno})",
                std::io::Error::from_raw_os_error(*errno)
            ),
            Self::NativeContract { operation, result } => {
                write!(
                    formatter,
                    "native {operation} returned invalid result {result}"
                )
            }
        }
    }
}

impl SerialPort {
    pub(in crate::application) fn open(path: &CString, baud: u32) -> Result<Self, SerialError> {
        let descriptor = unsafe { native::dmc2_serial_open(path.as_ptr(), baud) };
        if descriptor < 0 {
            Err(SerialError::from_failure("open", descriptor))
        } else {
            Ok(Self(descriptor))
        }
    }

    pub(in crate::application) fn read(&self, buffer: &mut [u8]) -> Result<usize, SerialError> {
        let count = unsafe { native::dmc2_serial_read(self.0, buffer.as_mut_ptr(), buffer.len()) };
        if count < 0 {
            Err(SerialError::from_failure("read", count))
        } else if count as usize > buffer.len() {
            Err(SerialError::NativeContract {
                operation: "read",
                result: count,
            })
        } else {
            Ok(count as usize)
        }
    }
}

impl Drop for SerialPort {
    fn drop(&mut self) {
        let result = unsafe { native::dmc2_serial_close(self.0) };
        if result != 0 {
            let error = SerialError::from_failure("close", result);
            eprintln!("dmc2-serial-bridge: serial close failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::{c_char, c_void, CStr};
    use std::fs::File;
    use std::io::Write;
    use std::mem::ManuallyDrop;
    use std::os::fd::FromRawFd;
    use std::ptr;

    use super::*;

    #[repr(C)]
    struct PollFd {
        fd: c_int,
        events: i16,
        revents: i16,
    }

    unsafe extern "C" {
        fn openpty(
            master: *mut c_int,
            slave: *mut c_int,
            name: *mut c_char,
            termios: *const c_void,
            window_size: *const c_void,
        ) -> c_int;
        fn poll(descriptors: *mut PollFd, count: usize, timeout_ms: c_int) -> c_int;
    }

    fn pseudo_terminal() -> (File, CString) {
        let mut master = -1;
        let mut slave = -1;
        let mut name = [0 as c_char; 256];
        assert_eq!(
            unsafe {
                openpty(
                    &mut master,
                    &mut slave,
                    name.as_mut_ptr(),
                    ptr::null(),
                    ptr::null(),
                )
            },
            0
        );
        let path = unsafe { CStr::from_ptr(name.as_ptr()) }.to_owned();
        let master = unsafe { File::from_raw_fd(master) };
        drop(unsafe { File::from_raw_fd(slave) });
        (master, path)
    }

    #[test]
    fn invalid_paths_bauds_descriptors_and_capacities_fail_closed() {
        let missing = CString::new("/definitely/not/a/dmc2/serial/device").unwrap();
        assert_eq!(
            SerialPort::open(&missing, 115_200).err(),
            Some(SerialError::OperatingSystem(2))
        );
        let not_a_terminal = CString::new("/dev/null").unwrap();
        assert_eq!(
            SerialPort::open(&not_a_terminal, 115_200).err(),
            Some(SerialError::OperatingSystem(25))
        );

        let (_master, terminal) = pseudo_terminal();
        assert_eq!(
            SerialPort::open(&terminal, 9_600).err(),
            Some(SerialError::OperatingSystem(22))
        );
        let empty = CString::new("").unwrap();
        assert_eq!(
            unsafe { native::dmc2_serial_open(ptr::null(), 115_200) },
            -22
        );
        assert_eq!(
            unsafe { native::dmc2_serial_open(empty.as_ptr(), 115_200) },
            -22
        );

        let invalid = ManuallyDrop::new(SerialPort(-1));
        let mut byte = [0_u8; 1];
        assert_eq!(
            invalid.read(&mut byte),
            Err(SerialError::OperatingSystem(22))
        );
        assert_eq!(invalid.read(&mut []), Err(SerialError::OperatingSystem(22)));
        assert_eq!(
            unsafe { native::dmc2_serial_read(0, ptr::null_mut(), 1) },
            -22
        );
        assert_eq!(unsafe { native::dmc2_serial_close(-1) }, -22);
    }

    #[test]
    fn native_close_reports_success_and_the_exact_repeat_close_failure() {
        let (_master, terminal) = pseudo_terminal();
        let descriptor = unsafe { native::dmc2_serial_open(terminal.as_ptr(), 115_200) };
        assert!(descriptor >= 0);
        assert_eq!(unsafe { native::dmc2_serial_close(descriptor) }, 0);
        assert_eq!(unsafe { native::dmc2_serial_close(descriptor) }, -9);
    }

    #[test]
    fn native_raw_serial_path_preserves_every_byte_value_exactly() {
        let (mut master, terminal) = pseudo_terminal();
        let serial = SerialPort::open(&terminal, 115_200).expect("pseudo-terminal opened");
        let mut empty = [0_u8; 1];
        assert_eq!(serial.read(&mut empty), Ok(0));

        let expected = (u8::MIN..=u8::MAX).collect::<Vec<_>>();
        master.write_all(&expected).unwrap();
        master.flush().unwrap();

        let mut descriptor = PollFd {
            fd: serial.0,
            events: 1,
            revents: 0,
        };
        assert_eq!(unsafe { poll(&mut descriptor, 1, 1_000) }, 1);
        assert_ne!(descriptor.revents & 1, 0);

        let mut actual = Vec::with_capacity(expected.len());
        while actual.len() < expected.len() {
            let mut buffer = [0_u8; 256];
            let count = serial.read(&mut buffer).expect("native serial read");
            assert!(
                count > 0,
                "poll reported readable but read returned no bytes"
            );
            actual.extend_from_slice(&buffer[..count]);
        }
        assert_eq!(actual, expected);
    }
}
