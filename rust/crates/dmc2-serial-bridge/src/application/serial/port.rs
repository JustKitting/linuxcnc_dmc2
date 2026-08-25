use std::ffi::{c_int, CString};

use super::posix;

pub(in crate::application) struct SerialPort(c_int);

impl SerialPort {
    pub(in crate::application) fn open(path: &CString, baud: u32) -> Result<Self, posix::Error> {
        posix::open(path.as_c_str(), baud).map(Self)
    }

    pub(in crate::application) fn read(&self, buffer: &mut [u8]) -> Result<usize, posix::Error> {
        posix::read(self.0, buffer)
    }
}

impl Drop for SerialPort {
    fn drop(&mut self) {
        if let Err(error) = posix::close(self.0) {
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

    use super::super::ffi::inspection as ffi;
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
            Some(posix::Error::OperatingSystem(2))
        );
        let not_a_terminal = CString::new("/dev/null").unwrap();
        assert_eq!(
            SerialPort::open(&not_a_terminal, 115_200).err(),
            Some(posix::Error::OperatingSystem(25))
        );

        let (_master, terminal) = pseudo_terminal();
        assert_eq!(
            SerialPort::open(&terminal, 9_600).err(),
            Some(posix::Error::OperatingSystem(22))
        );
        let empty = CString::new("").unwrap();
        assert_eq!(
            posix::open(empty.as_c_str(), 115_200),
            Err(posix::Error::OperatingSystem(22))
        );

        let invalid = ManuallyDrop::new(SerialPort(-1));
        let mut byte = [0_u8; 1];
        assert_eq!(
            invalid.read(&mut byte),
            Err(posix::Error::OperatingSystem(22))
        );
        assert_eq!(
            invalid.read(&mut []),
            Err(posix::Error::OperatingSystem(22))
        );
        assert_eq!(posix::close(-1), Err(posix::Error::OperatingSystem(22)));
    }

    #[test]
    fn posix_close_reports_success_and_the_exact_repeat_close_failure() {
        let (_master, terminal) = pseudo_terminal();
        let descriptor = posix::open(terminal.as_c_str(), 115_200).unwrap();
        assert_eq!(posix::close(descriptor), Ok(()));
        assert_eq!(
            posix::close(descriptor),
            Err(posix::Error::OperatingSystem(9))
        );
    }

    #[test]
    fn rust_posix_configuration_and_raw_serial_path_are_exact() {
        let (mut master, terminal) = pseudo_terminal();
        let serial = SerialPort::open(&terminal, 115_200).expect("pseudo-terminal opened");

        let mut options = unsafe { std::mem::zeroed::<ffi::termios>() };
        assert_eq!(unsafe { ffi::tcgetattr(serial.0, &mut options) }, 0);
        assert_ne!(options.c_cflag & ffi::CLOCAL as ffi::tcflag_t, 0);
        assert_ne!(options.c_cflag & ffi::CREAD as ffi::tcflag_t, 0);
        assert_eq!(options.c_cflag & ffi::CSTOPB as ffi::tcflag_t, 0);
        assert_eq!(options.c_cflag & ffi::CRTSCTS as ffi::tcflag_t, 0);
        assert_eq!(
            options.c_cflag & ffi::CSIZE as ffi::tcflag_t,
            ffi::CS8 as ffi::tcflag_t
        );
        assert_eq!(options.c_cc[ffi::VMIN as usize], 0);
        assert_eq!(options.c_cc[ffi::VTIME as usize], 0);
        assert_eq!(unsafe { ffi::cfgetispeed(&options) }, ffi::B115200);
        assert_eq!(unsafe { ffi::cfgetospeed(&options) }, ffi::B115200);
        let status_flags = unsafe { ffi::fcntl(serial.0, ffi::F_GETFL as c_int) };
        assert!(status_flags >= 0);
        assert_ne!(status_flags & ffi::O_NONBLOCK as c_int, 0);
        let descriptor_flags = unsafe { ffi::fcntl(serial.0, ffi::F_GETFD as c_int) };
        assert!(descriptor_flags >= 0);
        assert_ne!(descriptor_flags & ffi::FD_CLOEXEC as c_int, 0);

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
            let count = serial.read(&mut buffer).expect("Rust POSIX serial read");
            assert!(
                count > 0,
                "poll reported readable but read returned no bytes"
            );
            actual.extend_from_slice(&buffer[..count]);
        }
        assert_eq!(actual, expected);
    }
}
