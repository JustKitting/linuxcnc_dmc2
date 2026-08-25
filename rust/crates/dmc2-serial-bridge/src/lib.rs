pub const BOOT_MARKER: &str = "BOOT,P3,MYST1474-001,MONITOR_ONLY";
pub const MAX_SERIAL_LINE_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum AxisCode {
    Invalid = -2,
    Off = -1,
    X = 0,
    Y = 1,
    Z = 2,
    Axis4 = 3,
    Axis5 = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum MultiplierCode {
    Invalid = -1,
    Off = 0,
    X1 = 1,
    X10 = 10,
    X100 = 100,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Packet {
    pub sequence: u32,
    pub milliseconds: u32,
    pub detent_count: i32,
    pub transition_count: i32,
    pub quadrature_errors: u32,
    pub latest_detent: i32,
    pub axis: AxisCode,
    pub multiplier: MultiplierCode,
    pub deadman_held: bool,
    pub estop_pressed: bool,
    pub selector_valid: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    WrongFieldCount,
    WrongMarker,
    InvalidInteger,
    InvalidDetent,
    InvalidAxis,
    InvalidMultiplier,
    InvalidBoolean,
    RepeatedOrReversedSequence,
    UnexpectedBootMarker,
    NonAscii,
    OverlongLine,
}

fn parse_bool(token: &str) -> Result<bool, ProtocolError> {
    match token {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(ProtocolError::InvalidBoolean),
    }
}

fn parse_axis(token: &str) -> Result<AxisCode, ProtocolError> {
    match token {
        "X" => Ok(AxisCode::X),
        "Y" => Ok(AxisCode::Y),
        "Z" => Ok(AxisCode::Z),
        "4" => Ok(AxisCode::Axis4),
        "5" => Ok(AxisCode::Axis5),
        "N" => Ok(AxisCode::Off),
        "I" => Ok(AxisCode::Invalid),
        _ => Err(ProtocolError::InvalidAxis),
    }
}

fn parse_multiplier(token: &str) -> Result<MultiplierCode, ProtocolError> {
    match token {
        "X1" => Ok(MultiplierCode::X1),
        "X10" => Ok(MultiplierCode::X10),
        "X100" => Ok(MultiplierCode::X100),
        "N" => Ok(MultiplierCode::Off),
        "I" => Ok(MultiplierCode::Invalid),
        _ => Err(ProtocolError::InvalidMultiplier),
    }
}

pub fn parse_packet(line: &str) -> Result<Packet, ProtocolError> {
    if line.len() > MAX_SERIAL_LINE_BYTES {
        return Err(ProtocolError::OverlongLine);
    }
    if !line.is_ascii() {
        return Err(ProtocolError::NonAscii);
    }
    let mut fields = line.trim().split(',');
    let marker = fields.next().ok_or(ProtocolError::WrongFieldCount)?;
    let sequence = fields.next().ok_or(ProtocolError::WrongFieldCount)?;
    let milliseconds = fields.next().ok_or(ProtocolError::WrongFieldCount)?;
    let detent_count = fields.next().ok_or(ProtocolError::WrongFieldCount)?;
    let transition_count = fields.next().ok_or(ProtocolError::WrongFieldCount)?;
    let quadrature_errors = fields.next().ok_or(ProtocolError::WrongFieldCount)?;
    let latest_detent = fields.next().ok_or(ProtocolError::WrongFieldCount)?;
    let axis = fields.next().ok_or(ProtocolError::WrongFieldCount)?;
    let multiplier = fields.next().ok_or(ProtocolError::WrongFieldCount)?;
    let deadman_held = fields.next().ok_or(ProtocolError::WrongFieldCount)?;
    let estop_pressed = fields.next().ok_or(ProtocolError::WrongFieldCount)?;
    let selector_valid = fields.next().ok_or(ProtocolError::WrongFieldCount)?;
    if fields.next().is_some() {
        return Err(ProtocolError::WrongFieldCount);
    }
    if marker != "P3" {
        return Err(ProtocolError::WrongMarker);
    }
    let parse_u32 = |value: &str| {
        value
            .parse::<u32>()
            .map_err(|_| ProtocolError::InvalidInteger)
    };
    let parse_i32 = |value: &str| {
        value
            .parse::<i32>()
            .map_err(|_| ProtocolError::InvalidInteger)
    };
    let latest_detent = parse_i32(latest_detent)?;
    if !(-1..=1).contains(&latest_detent) {
        return Err(ProtocolError::InvalidDetent);
    }
    Ok(Packet {
        sequence: parse_u32(sequence)?,
        milliseconds: parse_u32(milliseconds)?,
        detent_count: parse_i32(detent_count)?,
        transition_count: parse_i32(transition_count)?,
        quadrature_errors: parse_u32(quadrature_errors)?,
        latest_detent,
        axis: parse_axis(axis)?,
        multiplier: parse_multiplier(multiplier)?,
        deadman_held: parse_bool(deadman_held)?,
        estop_pressed: parse_bool(estop_pressed)?,
        selector_valid: parse_bool(selector_valid)?,
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;

    const IDLE: &str = "P3,1,20,0,0,0,0,X,X1,0,0,1";

    #[test]
    fn parses_exact_p3_packet() {
        let packet = parse_packet("P3,2,40,-1,4,0,1,Z,X100,1,0,1").unwrap();
        assert_eq!(packet.sequence, 2);
        assert_eq!(packet.detent_count, -1);
        assert_eq!(packet.axis, AxisCode::Z);
        assert_eq!(packet.multiplier, MultiplierCode::X100);
        assert!(packet.deadman_held);
        assert_eq!(packet.latest_detent, 1);
    }

    #[test]
    fn first_packet_never_publishes_a_detent() {
        let mut state = BridgeState::new(100_000_000);
        state
            .accept_line(b"P3,1,20,1,4,0,1,X,X1,1,0,1", 20_000_000)
            .unwrap();
        assert!(state.snapshot.connected);
        assert_eq!(state.snapshot.latest_detent, 0);
        state
            .accept_line(b"P3,2,40,2,8,0,1,X,X1,1,0,1", 40_000_000)
            .unwrap();
        assert_eq!(state.snapshot.latest_detent, 1);
    }

    #[test]
    fn sequence_gap_counts_loss_without_creating_a_queue() {
        let mut state = BridgeState::new(100_000_000);
        state.accept_line(IDLE.as_bytes(), 20_000_000).unwrap();
        state
            .accept_line(b"P3,1000001,40,9,9,0,-1,Y,X10,1,0,1", 40_000_000)
            .unwrap();
        assert_eq!(state.snapshot.dropped_packets, 999_999);
        assert_eq!(state.snapshot.latest_detent, -1);
    }

    #[test]
    fn timeout_and_protocol_error_publish_safe_state() {
        let mut state = BridgeState::new(100_000_000);
        state.accept_line(IDLE.as_bytes(), 0).unwrap();
        assert!(!state.check_timeout(100_000_000));
        assert!(state.check_timeout(100_000_001));
        assert!(!state.snapshot.connected);
        assert!(state.snapshot.serial_fault);
        assert!(state.snapshot.estop_pressed);

        assert!(state.accept_line(b"P3,broken", 200_000_000).is_err());
        assert_eq!(state.snapshot.protocol_errors, 1);
        assert!(state.snapshot.estop_pressed);
    }

    #[test]
    fn quadrature_error_latches_and_suppresses_detents() {
        let mut state = BridgeState::new(100_000_000);
        state.accept_line(IDLE.as_bytes(), 0).unwrap();
        state
            .accept_line(b"P3,2,20,1,4,1,1,X,X1,1,0,1", 20_000_000)
            .unwrap();
        assert!(state.snapshot.quadrature_fault);
        assert_eq!(state.snapshot.latest_detent, 0);
        assert!(!state.snapshot.link_healthy);
    }
}
