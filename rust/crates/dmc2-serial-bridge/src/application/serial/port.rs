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
