use std::ffi::{c_int, CString};

use super::native;

pub(in crate::application) struct SerialPort(c_int);

impl SerialPort {
    pub(in crate::application) fn open(path: &CString, baud: u32) -> Option<Self> {
        let descriptor = unsafe { native::dmc2_serial_open(path.as_ptr(), baud) };
        (descriptor >= 0).then_some(Self(descriptor))
    }

    pub(in crate::application) fn read(&self, buffer: &mut [u8]) -> Result<usize, ()> {
        let count = unsafe { native::dmc2_serial_read(self.0, buffer.as_mut_ptr(), buffer.len()) };
        if count < 0 || count as usize > buffer.len() {
            Err(())
        } else {
            Ok(count as usize)
        }
    }
}

impl Drop for SerialPort {
    fn drop(&mut self) {
        unsafe { native::dmc2_serial_close(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::{c_char, c_void, CStr};
    use std::fs::File;
    use std::io::Write;
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
        assert!(SerialPort::open(&missing, 115_200).is_none());
        let not_a_terminal = CString::new("/dev/null").unwrap();
        assert!(SerialPort::open(&not_a_terminal, 115_200).is_none());

        let (_master, terminal) = pseudo_terminal();
        assert!(SerialPort::open(&terminal, 9_600).is_none());
        let empty = CString::new("").unwrap();
        assert_eq!(
            unsafe { native::dmc2_serial_open(ptr::null(), 115_200) },
            -1
        );
        assert_eq!(
            unsafe { native::dmc2_serial_open(empty.as_ptr(), 115_200) },
            -1
        );

        let invalid = SerialPort(-1);
        let mut byte = [0_u8; 1];
        assert_eq!(invalid.read(&mut byte), Err(()));
        assert_eq!(invalid.read(&mut []), Err(()));
        assert_eq!(
            unsafe { native::dmc2_serial_read(0, ptr::null_mut(), 1) },
            -1
        );
        unsafe { native::dmc2_serial_close(-1) };
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
