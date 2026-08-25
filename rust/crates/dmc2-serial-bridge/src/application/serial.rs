use std::ffi::{c_char, c_int, c_uint, CString};

use dmc2_serial_bridge::{BridgeState, MAX_SERIAL_LINE_BYTES};

unsafe extern "C" {
    fn dmc2_serial_open(path: *const c_char, baud: c_uint) -> c_int;
    fn dmc2_serial_read(fd: c_int, buffer: *mut u8, capacity: usize) -> c_int;
    fn dmc2_serial_close(fd: c_int);
}

pub(super) struct SerialPort(c_int);

impl SerialPort {
    pub(super) fn open(path: &CString, baud: u32) -> Option<Self> {
        let descriptor = unsafe { dmc2_serial_open(path.as_ptr(), baud) };
        (descriptor >= 0).then_some(Self(descriptor))
    }

    pub(super) fn read(&self, buffer: &mut [u8]) -> Result<usize, ()> {
        let count = unsafe { dmc2_serial_read(self.0, buffer.as_mut_ptr(), buffer.len()) };
        if count < 0 {
            Err(())
        } else {
            Ok(count as usize)
        }
    }
}

impl Drop for SerialPort {
    fn drop(&mut self) {
        unsafe { dmc2_serial_close(self.0) };
    }
}

pub(super) struct LineAssembler {
    bytes: [u8; MAX_SERIAL_LINE_BYTES],
    length: usize,
    overlong: bool,
}

impl LineAssembler {
    pub(super) const fn new() -> Self {
        Self {
            bytes: [0; MAX_SERIAL_LINE_BYTES],
            length: 0,
            overlong: false,
        }
    }

    pub(super) fn consume(&mut self, byte: u8, state: &mut BridgeState, now_ns: u64) -> bool {
        if byte == b'\r' {
            return false;
        }
        if byte != b'\n' {
            if self.length < self.bytes.len() {
                self.bytes[self.length] = byte;
                self.length += 1;
            } else {
                self.overlong = true;
            }
            return false;
        }

        if self.overlong {
            state.note_protocol_error();
        } else if self.length > 0 {
            let _ = state.accept_line(&self.bytes[..self.length], now_ns);
        }
        self.length = 0;
        self.overlong = false;
        true
    }
}
