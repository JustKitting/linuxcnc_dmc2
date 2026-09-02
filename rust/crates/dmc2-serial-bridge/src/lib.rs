//! Typed boundary between the Nano P3 wire protocol and LinuxCNC HAL.

mod bridge_fault;
#[doc(hidden)]
#[path = "application/hal/mod.rs"]
pub mod hal;
mod protocol;
mod state;

pub use bridge_fault::{BridgeFaultCode, BridgeFaultEvidence, BridgeFaultRecord};
pub use protocol::{
    parse_packet, AxisCode, MultiplierCode, Packet, ProtocolError, BOOT_MARKER,
    MAX_SERIAL_LINE_BYTES,
};
pub use state::{BridgeState, ProtocolErrorEvidence, ProtocolErrorRecord, Snapshot};
