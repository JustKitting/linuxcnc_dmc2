mod ffi;
mod framing;
mod port;
mod posix;

pub(super) use framing::{LineAssembler, LineEvent};
pub(super) use port::SerialPort;
