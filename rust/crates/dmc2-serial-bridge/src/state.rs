use crate::{
    parse_packet, AxisCode, BridgeFaultCode, BridgeFaultEvidence, BridgeFaultRecord,
    MultiplierCode, Packet, ProtocolError, BOOT_MARKER, MAX_SERIAL_LINE_BYTES,
};

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolErrorEvidence {
    pub line_bytes: Option<u32>,
    pub previous_sequence: Option<u32>,
    pub observed_sequence: Option<u32>,
}

impl ProtocolErrorEvidence {
    pub const fn empty() -> Self {
        Self {
            line_bytes: None,
            previous_sequence: None,
            observed_sequence: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolErrorRecord {
    pub code: ProtocolError,
    pub evidence: ProtocolErrorEvidence,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
    pub fault_reset_ack: u32,
    pub connected: bool,
    pub serial_fault: bool,
    pub quadrature_fault: bool,
    pub link_healthy: bool,
    pub heartbeat: bool,
    pub estop_pressed: bool,
    pub deadman_held: bool,
    pub selector_valid: bool,
    pub axis: AxisCode,
    pub multiplier: MultiplierCode,
    pub latest_detent: i32,
    pub detent_count: i32,
    pub transition_count: i32,
    pub quadrature_errors: u32,
    pub sequence: u32,
    pub milliseconds: u32,
    pub protocol_errors: u32,
    pub last_protocol_error: Option<ProtocolErrorRecord>,
    pub current_fault: Option<BridgeFaultRecord>,
    pub dropped_packets: u32,
    pub timeouts: u32,
}

impl Snapshot {
    pub const fn safe() -> Self {
        Self {
            fault_reset_ack: 0,
            connected: false,
            serial_fault: true,
            quadrature_fault: false,
            link_healthy: false,
            heartbeat: false,
            estop_pressed: true,
            deadman_held: false,
            selector_valid: false,
            axis: AxisCode::Invalid,
            multiplier: MultiplierCode::Invalid,
            latest_detent: 0,
            detent_count: 0,
            transition_count: 0,
            quadrature_errors: 0,
            sequence: 0,
            milliseconds: 0,
            protocol_errors: 0,
            last_protocol_error: None,
            current_fault: Some(BridgeFaultRecord::empty(
                BridgeFaultCode::AwaitingFirstPacket,
            )),
            dropped_packets: 0,
            timeouts: 0,
        }
    }
}

impl Default for Snapshot {
    fn default() -> Self {
        Self::safe()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PendingFaultReset {
    request: u32,
    quadrature_errors: u32,
    requested_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BridgeState {
    pub snapshot: Snapshot,
    timeout_ns: u64,
    last_packet_ns: Option<u64>,
    previous_sequence: Option<u32>,
    previous_quadrature_errors: Option<u32>,
    latched_quadrature_fault: Option<BridgeFaultRecord>,
    baseline_required: bool,
    last_reset_request: u32,
    pending_reset: Option<PendingFaultReset>,
}

impl BridgeState {
    pub const fn new(timeout_ns: u64) -> Self {
        Self {
            snapshot: Snapshot::safe(),
            timeout_ns,
            last_packet_ns: None,
            previous_sequence: None,
            previous_quadrature_errors: None,
            latched_quadrature_fault: None,
            baseline_required: true,
            last_reset_request: 0,
            pending_reset: None,
        }
    }

    /// Zero cancels a request; a nonzero generation represents one explicit
    /// operator reset. It is consumed once, never retried automatically.
    pub fn observe_fault_reset(&mut self, request: u32, now_ns: u64) {
        if request == 0 {
            self.pending_reset = None;
            return;
        }
        if request == self.last_reset_request {
            return;
        }
        self.last_reset_request = request;
        self.pending_reset = None;
        let fresh = self
            .last_packet_ns
            .is_some_and(|last| now_ns.saturating_sub(last) <= self.timeout_ns);
        if fresh
            && !self.baseline_required
            && self.snapshot.connected
            && !self.snapshot.serial_fault
            && self.snapshot.quadrature_fault
            && !self.snapshot.estop_pressed
            && !self.snapshot.deadman_held
        {
            self.pending_reset = Some(PendingFaultReset {
                request,
                quadrature_errors: self.snapshot.quadrature_errors,
                requested_ns: now_ns,
            });
        }
    }

    fn safe_snapshot(&self, fault: BridgeFaultRecord) -> Snapshot {
        Snapshot {
            serial_fault: fault.code.is_serial_fault(),
            quadrature_fault: self.snapshot.quadrature_fault,
            heartbeat: self.snapshot.heartbeat,
            estop_pressed: true,
            protocol_errors: self.snapshot.protocol_errors,
            last_protocol_error: self.snapshot.last_protocol_error,
            current_fault: Some(fault),
            dropped_packets: self.snapshot.dropped_packets,
            timeouts: self.snapshot.timeouts,
            quadrature_errors: self.snapshot.quadrature_errors,
            ..Snapshot::safe()
        }
    }

    pub fn reset_for_boot(&mut self) {
        self.pending_reset = None;
        let heartbeat = self.snapshot.heartbeat;
        let protocol_errors = self.snapshot.protocol_errors;
        let last_protocol_error = self.snapshot.last_protocol_error;
        let dropped_packets = self.snapshot.dropped_packets;
        let timeouts = self.snapshot.timeouts;
        self.snapshot = Snapshot {
            heartbeat,
            protocol_errors,
            last_protocol_error,
            current_fault: Some(BridgeFaultRecord::empty(
                BridgeFaultCode::AwaitingFirstPacket,
            )),
            dropped_packets,
            timeouts,
            ..Snapshot::safe()
        };
        self.last_packet_ns = None;
        self.previous_sequence = None;
        self.previous_quadrature_errors = None;
        self.latched_quadrature_fault = None;
        self.baseline_required = true;
    }

    fn note_transport_fault(
        &mut self,
        code: BridgeFaultCode,
        operating_system_error: Option<i32>,
        transport_contract_result: Option<i64>,
    ) {
        self.pending_reset = None;
        let fault = BridgeFaultRecord::new(
            code,
            BridgeFaultEvidence {
                operating_system_error,
                transport_contract_result,
                ..BridgeFaultEvidence::empty()
            },
        );
        self.snapshot = self.safe_snapshot(fault);
        self.last_packet_ns = None;
        self.previous_sequence = None;
        self.previous_quadrature_errors = None;
        self.baseline_required = true;
    }

    pub fn note_serial_open_failure(
        &mut self,
        operating_system_error: Option<i32>,
        transport_contract_result: Option<i64>,
    ) {
        self.note_transport_fault(
            BridgeFaultCode::SerialOpenFailure,
            operating_system_error,
            transport_contract_result,
        );
    }

    pub fn note_serial_read_failure(
        &mut self,
        operating_system_error: Option<i32>,
        transport_contract_result: Option<i64>,
    ) {
        self.note_transport_fault(
            BridgeFaultCode::SerialReadFailure,
            operating_system_error,
            transport_contract_result,
        );
    }

    pub fn note_protocol_error(&mut self, error: ProtocolError, line_bytes: Option<usize>) {
        self.note_protocol_error_with_evidence(
            error,
            ProtocolErrorEvidence {
                line_bytes: line_bytes.map(|value| value.min(u32::MAX as usize) as u32),
                ..ProtocolErrorEvidence::empty()
            },
        );
    }

    fn note_protocol_error_with_evidence(
        &mut self,
        error: ProtocolError,
        evidence: ProtocolErrorEvidence,
    ) {
        let next = self.snapshot.protocol_errors.wrapping_add(1);
        let protocol_record = ProtocolErrorRecord {
            code: error,
            evidence,
        };
        self.pending_reset = None;
        let fault = BridgeFaultRecord::new(
            BridgeFaultCode::ProtocolRejected,
            BridgeFaultEvidence {
                protocol_error: Some(error),
                line_bytes: evidence.line_bytes,
                previous_sequence: evidence.previous_sequence,
                observed_sequence: evidence.observed_sequence,
                ..BridgeFaultEvidence::empty()
            },
        );
        self.snapshot = Snapshot {
            protocol_errors: next,
            last_protocol_error: Some(protocol_record),
            ..self.safe_snapshot(fault)
        };
        self.last_packet_ns = None;
        self.previous_sequence = None;
        self.previous_quadrature_errors = None;
        self.baseline_required = true;
    }

    pub fn accept(&mut self, packet: Packet, now_ns: u64) -> Result<(), ProtocolError> {
        self.accept_with_line_length(packet, now_ns, None)
    }

    fn accept_with_line_length(
        &mut self,
        packet: Packet,
        now_ns: u64,
        line_bytes: Option<usize>,
    ) -> Result<(), ProtocolError> {
        if let Some(previous) = self.previous_sequence {
            let delta = packet.sequence.wrapping_sub(previous);
            if delta == 0 || delta > i32::MAX as u32 {
                self.note_protocol_error_with_evidence(
                    ProtocolError::RepeatedOrReversedSequence,
                    ProtocolErrorEvidence {
                        line_bytes: line_bytes.map(|value| value.min(u32::MAX as usize) as u32),
                        previous_sequence: Some(previous),
                        observed_sequence: Some(packet.sequence),
                    },
                );
                return Err(ProtocolError::RepeatedOrReversedSequence);
            }
            if delta > 1 {
                self.snapshot.dropped_packets =
                    self.snapshot.dropped_packets.wrapping_add(delta - 1);
            }
        }

        let first_packet = self.baseline_required;
        let mut quadrature_fault = self.snapshot.quadrature_fault;
        let pending_reset = self.pending_reset.take();
        let reset_accepted = pending_reset.is_some_and(|reset| {
            !first_packet
                && self.snapshot.connected
                && !self.snapshot.serial_fault
                && !packet.estop_pressed
                && !packet.deadman_held
                && now_ns.saturating_sub(reset.requested_ns) <= self.timeout_ns
                && packet.quadrature_errors == reset.quadrature_errors
                && self.previous_quadrature_errors == Some(reset.quadrature_errors)
        });
        if reset_accepted {
            quadrature_fault = false;
            self.latched_quadrature_fault = None;
        }
        if let Some(previous) = self.previous_quadrature_errors {
            if packet.quadrature_errors != previous {
                quadrature_fault = true;
                self.latched_quadrature_fault = Some(BridgeFaultRecord::new(
                    BridgeFaultCode::QuadratureCounterChanged,
                    BridgeFaultEvidence {
                        previous_quadrature_errors: Some(previous),
                        observed_quadrature_errors: Some(packet.quadrature_errors),
                        ..BridgeFaultEvidence::empty()
                    },
                ));
            }
        }
        let latest_detent = if first_packet || quadrature_fault || reset_accepted {
            0
        } else {
            packet.latest_detent
        };
        self.snapshot = Snapshot {
            fault_reset_ack: pending_reset
                .filter(|_| reset_accepted)
                .map_or(self.snapshot.fault_reset_ack, |reset| reset.request),
            connected: true,
            serial_fault: false,
            quadrature_fault,
            link_healthy: !quadrature_fault && !packet.estop_pressed,
            heartbeat: !self.snapshot.heartbeat,
            estop_pressed: packet.estop_pressed,
            deadman_held: packet.deadman_held,
            selector_valid: packet.selector_valid,
            axis: packet.axis,
            multiplier: packet.multiplier,
            latest_detent,
            detent_count: packet.detent_count,
            transition_count: packet.transition_count,
            quadrature_errors: packet.quadrature_errors,
            sequence: packet.sequence,
            milliseconds: packet.milliseconds,
            protocol_errors: self.snapshot.protocol_errors,
            last_protocol_error: self.snapshot.last_protocol_error,
            current_fault: if quadrature_fault {
                self.latched_quadrature_fault
                    .or(Some(BridgeFaultRecord::new(
                        BridgeFaultCode::QuadratureCounterChanged,
                        BridgeFaultEvidence {
                            observed_quadrature_errors: Some(packet.quadrature_errors),
                            ..BridgeFaultEvidence::empty()
                        },
                    )))
            } else {
                None
            },
            dropped_packets: self.snapshot.dropped_packets,
            timeouts: self.snapshot.timeouts,
        };
        self.last_packet_ns = Some(now_ns);
        self.previous_sequence = Some(packet.sequence);
        self.previous_quadrature_errors = Some(packet.quadrature_errors);
        self.baseline_required = false;
        Ok(())
    }

    pub fn accept_line(&mut self, bytes: &[u8], now_ns: u64) -> Result<(), ProtocolError> {
        if bytes.len() > MAX_SERIAL_LINE_BYTES {
            self.note_protocol_error(ProtocolError::OverlongLine, Some(bytes.len()));
            return Err(ProtocolError::OverlongLine);
        }
        let line = match core::str::from_utf8(bytes) {
            Ok(value) if value.is_ascii() => value,
            _ => {
                self.note_protocol_error(ProtocolError::NonAscii, Some(bytes.len()));
                return Err(ProtocolError::NonAscii);
            }
        };
        if line == BOOT_MARKER {
            self.reset_for_boot();
            return Ok(());
        }
        if line.starts_with("BOOT,") {
            self.note_protocol_error(ProtocolError::UnexpectedBootMarker, Some(bytes.len()));
            return Err(ProtocolError::UnexpectedBootMarker);
        }
        match parse_packet(line) {
            Ok(packet) => self.accept_with_line_length(packet, now_ns, Some(bytes.len())),
            Err(error) => {
                self.note_protocol_error(error, Some(bytes.len()));
                Err(error)
            }
        }
    }

    pub fn check_timeout(&mut self, now_ns: u64) -> bool {
        let Some(last_packet_ns) = self.last_packet_ns else {
            return false;
        };
        let packet_age_ns = now_ns.saturating_sub(last_packet_ns);
        if packet_age_ns <= self.timeout_ns {
            return false;
        }
        let timeouts = self.snapshot.timeouts.wrapping_add(1);
        self.pending_reset = None;
        let fault = BridgeFaultRecord::new(
            BridgeFaultCode::PacketTimeout,
            BridgeFaultEvidence {
                packet_age_ns: Some(packet_age_ns),
                timeout_ns: Some(self.timeout_ns),
                ..BridgeFaultEvidence::empty()
            },
        );
        self.snapshot = Snapshot {
            timeouts,
            ..self.safe_snapshot(fault)
        };
        self.last_packet_ns = None;
        self.previous_sequence = None;
        self.previous_quadrature_errors = None;
        self.baseline_required = true;
        true
    }

    pub fn packet_age_ms(&self, now_ns: u64) -> f64 {
        match self.last_packet_ns {
            Some(value) => now_ns.saturating_sub(value) as f64 / 1_000_000.0,
            None => -1.0,
        }
    }
}
