//! Typed boundary between the Nano P3 wire protocol and LinuxCNC HAL.

mod protocol;
mod state;

pub use protocol::{
    parse_packet, AxisCode, MultiplierCode, Packet, ProtocolError, BOOT_MARKER,
    MAX_SERIAL_LINE_BYTES,
};
pub use state::{BridgeState, Snapshot};

#[cfg(test)]
mod tests;
