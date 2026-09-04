use dmc2_diagnostics::{diagnostic_catalog, RecoveryClass, RecoveryClassified};

use crate::ProtocolError;

diagnostic_catalog! {
    pub enum BridgeFaultCode {
    AwaitingFirstPacket = 1,
    "AWAITING_FIRST_PACKET",
    "awaiting-first-packet",
    "the serial bridge has not yet accepted the baseline P3 packet required after startup or reset",
    "verify the Nano is connected and emitting the exact BOOT marker followed by fresh P3 packets";
    SerialOpenFailure = 2,
    "SERIAL_OPEN_FAILURE",
    "serial-open-failure",
    "the serial bridge could not open or configure the requested Nano serial device",
    "inspect the retained OS/contract result, serial device path, permissions, and 115200-baud device";
    SerialReadFailure = 3,
    "SERIAL_READ_FAILURE",
    "serial-read-failure",
    "an established Nano serial device returned a non-retryable read failure",
    "inspect the retained OS/contract result and USB/Nano connection before reconnecting";
    ProtocolRejected = 4,
    "PROTOCOL_REJECTED",
    "protocol-rejected",
    "the serial bridge rejected a complete line under the exact P3 protocol contract",
    "inspect the nested named protocol error and its retained line/sequence evidence";
    PacketTimeout = 5,
    "PACKET_TIMEOUT",
    "packet-timeout",
    "an established P3 stream stopped producing fresh packets before the configured deadline",
    "inspect retained packet age and timeout, then verify Nano power, USB, firmware, and serial output";
    QuadratureCounterChanged = 6,
    "QUADRATURE_COUNTER_CHANGED",
    "quadrature-counter-changed",
    "the Nano reported a changed quadrature-error counter after the accepted baseline",
    "inspect retained previous/current counters and the pendant wheel signals before restarting control";
    }
}

impl RecoveryClassified for BridgeFaultCode {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::AwaitingFirstPacket
            | Self::SerialOpenFailure
            | Self::SerialReadFailure
            | Self::ProtocolRejected
            | Self::PacketTimeout
            | Self::QuadratureCounterChanged => RecoveryClass::RestorePendant,
        }
    }
}

impl BridgeFaultCode {
    pub const fn is_serial_fault(self) -> bool {
        !matches!(self, Self::QuadratureCounterChanged)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeFaultEvidence {
    pub protocol_error: Option<ProtocolError>,
    pub line_bytes: Option<u32>,
    pub previous_sequence: Option<u32>,
    pub observed_sequence: Option<u32>,
    pub packet_age_ns: Option<u64>,
    pub timeout_ns: Option<u64>,
    pub previous_quadrature_errors: Option<u32>,
    pub observed_quadrature_errors: Option<u32>,
    pub operating_system_error: Option<i32>,
    pub transport_contract_result: Option<i64>,
}

impl BridgeFaultEvidence {
    pub const fn empty() -> Self {
        Self {
            protocol_error: None,
            line_bytes: None,
            previous_sequence: None,
            observed_sequence: None,
            packet_age_ns: None,
            timeout_ns: None,
            previous_quadrature_errors: None,
            observed_quadrature_errors: None,
            operating_system_error: None,
            transport_contract_result: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeFaultRecord {
    pub code: BridgeFaultCode,
    pub evidence: BridgeFaultEvidence,
}

impl BridgeFaultRecord {
    pub const fn new(code: BridgeFaultCode, evidence: BridgeFaultEvidence) -> Self {
        Self { code, evidence }
    }

    pub const fn empty(code: BridgeFaultCode) -> Self {
        Self::new(code, BridgeFaultEvidence::empty())
    }
}
