use crate::{
    parse_packet, AxisCode, MultiplierCode, Packet, ProtocolError, BOOT_MARKER,
    MAX_SERIAL_LINE_BYTES,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
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
    pub dropped_packets: u32,
    pub timeouts: u32,
}

impl Snapshot {
    pub const fn safe() -> Self {
        Self {
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
pub struct BridgeState {
    pub snapshot: Snapshot,
    timeout_ns: u64,
    last_packet_ns: Option<u64>,
    previous_sequence: Option<u32>,
    previous_quadrature_errors: Option<u32>,
    baseline_required: bool,
}

impl BridgeState {
    pub const fn new(timeout_ns: u64) -> Self {
        Self {
            snapshot: Snapshot::safe(),
            timeout_ns,
            last_packet_ns: None,
            previous_sequence: None,
            previous_quadrature_errors: None,
            baseline_required: true,
        }
    }

    fn safe_snapshot(&self, serial_fault: bool) -> Snapshot {
        Snapshot {
            serial_fault,
            quadrature_fault: self.snapshot.quadrature_fault,
            heartbeat: self.snapshot.heartbeat,
            estop_pressed: true,
            protocol_errors: self.snapshot.protocol_errors,
            dropped_packets: self.snapshot.dropped_packets,
            timeouts: self.snapshot.timeouts,
            quadrature_errors: self.snapshot.quadrature_errors,
            ..Snapshot::safe()
        }
    }

    pub fn reset_for_boot(&mut self) {
        let heartbeat = self.snapshot.heartbeat;
        let protocol_errors = self.snapshot.protocol_errors;
        let dropped_packets = self.snapshot.dropped_packets;
        let timeouts = self.snapshot.timeouts;
        self.snapshot = Snapshot {
            heartbeat,
            protocol_errors,
            dropped_packets,
            timeouts,
            ..Snapshot::safe()
        };
        self.last_packet_ns = None;
        self.previous_sequence = None;
        self.previous_quadrature_errors = None;
        self.baseline_required = true;
    }

    pub fn note_serial_fault(&mut self) {
        self.snapshot = self.safe_snapshot(true);
        self.last_packet_ns = None;
        self.previous_sequence = None;
        self.previous_quadrature_errors = None;
        self.baseline_required = true;
    }

    pub fn note_protocol_error(&mut self) {
        let next = self.snapshot.protocol_errors.wrapping_add(1);
        self.snapshot = Snapshot {
            protocol_errors: next,
            ..self.safe_snapshot(true)
        };
        self.last_packet_ns = None;
        self.previous_sequence = None;
        self.previous_quadrature_errors = None;
        self.baseline_required = true;
    }

    pub fn accept(&mut self, packet: Packet, now_ns: u64) -> Result<(), ProtocolError> {
        if let Some(previous) = self.previous_sequence {
            let delta = packet.sequence.wrapping_sub(previous);
            if delta == 0 || delta > i32::MAX as u32 {
                self.note_protocol_error();
                return Err(ProtocolError::RepeatedOrReversedSequence);
            }
            if delta > 1 {
                self.snapshot.dropped_packets =
                    self.snapshot.dropped_packets.wrapping_add(delta - 1);
            }
        }

        let first_packet = self.baseline_required;
        let mut quadrature_fault = self.snapshot.quadrature_fault;
        if let Some(previous) = self.previous_quadrature_errors {
            if packet.quadrature_errors != previous {
                quadrature_fault = true;
            }
        }
        let latest_detent = if first_packet || quadrature_fault {
            0
        } else {
            packet.latest_detent
        };
        self.snapshot = Snapshot {
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
            self.note_protocol_error();
            return Err(ProtocolError::OverlongLine);
        }
        let line = match core::str::from_utf8(bytes) {
            Ok(value) if value.is_ascii() => value.trim(),
            _ => {
                self.note_protocol_error();
                return Err(ProtocolError::NonAscii);
            }
        };
        if line == BOOT_MARKER {
            self.reset_for_boot();
            return Ok(());
        }
        if line.starts_with("BOOT,") {
            self.note_protocol_error();
            return Err(ProtocolError::UnexpectedBootMarker);
        }
        match parse_packet(line) {
            Ok(packet) => self.accept(packet, now_ns),
            Err(error) => {
                self.note_protocol_error();
                Err(error)
            }
        }
    }

    pub fn check_timeout(&mut self, now_ns: u64) -> bool {
        let Some(last_packet_ns) = self.last_packet_ns else {
            return false;
        };
        if now_ns.saturating_sub(last_packet_ns) <= self.timeout_ns {
            return false;
        }
        let timeouts = self.snapshot.timeouts.wrapping_add(1);
        self.snapshot = Snapshot {
            timeouts,
            ..self.safe_snapshot(true)
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
