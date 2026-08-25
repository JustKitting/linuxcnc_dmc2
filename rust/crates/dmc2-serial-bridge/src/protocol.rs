//! Parser and wire-level types for the Nano P3 protocol.

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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
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

impl ProtocolError {
    pub const ALL: [Self; 11] = [
        Self::WrongFieldCount,
        Self::WrongMarker,
        Self::InvalidInteger,
        Self::InvalidDetent,
        Self::InvalidAxis,
        Self::InvalidMultiplier,
        Self::InvalidBoolean,
        Self::RepeatedOrReversedSequence,
        Self::UnexpectedBootMarker,
        Self::NonAscii,
        Self::OverlongLine,
    ];
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

fn canonical_unsigned(token: &str) -> bool {
    token == "0"
        || token.as_bytes().split_first().is_some_and(|(first, rest)| {
            matches!(first, b'1'..=b'9') && rest.iter().all(u8::is_ascii_digit)
        })
}

fn parse_u32(token: &str) -> Result<u32, ProtocolError> {
    if !canonical_unsigned(token) {
        return Err(ProtocolError::InvalidInteger);
    }
    token
        .parse::<u32>()
        .map_err(|_| ProtocolError::InvalidInteger)
}

fn parse_i32(token: &str) -> Result<i32, ProtocolError> {
    let digits = token.strip_prefix('-').unwrap_or(token);
    if !canonical_unsigned(digits) || (token.starts_with('-') && digits == "0") {
        return Err(ProtocolError::InvalidInteger);
    }
    token
        .parse::<i32>()
        .map_err(|_| ProtocolError::InvalidInteger)
}

pub fn parse_packet(line: &str) -> Result<Packet, ProtocolError> {
    if line.len() > MAX_SERIAL_LINE_BYTES {
        return Err(ProtocolError::OverlongLine);
    }
    if !line.is_ascii() {
        return Err(ProtocolError::NonAscii);
    }
    let mut fields = line.split(',');
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
    let sequence = parse_u32(sequence)?;
    let milliseconds = parse_u32(milliseconds)?;
    let detent_count = parse_i32(detent_count)?;
    let transition_count = parse_i32(transition_count)?;
    let quadrature_errors = parse_u32(quadrature_errors)?;
    let latest_detent = parse_i32(latest_detent)?;
    if !(-1..=1).contains(&latest_detent) {
        return Err(ProtocolError::InvalidDetent);
    }
    Ok(Packet {
        sequence,
        milliseconds,
        detent_count,
        transition_count,
        quadrature_errors,
        latest_detent,
        axis: parse_axis(axis)?,
        multiplier: parse_multiplier(multiplier)?,
        deadman_held: parse_bool(deadman_held)?,
        estop_pressed: parse_bool(estop_pressed)?,
        selector_valid: parse_bool(selector_valid)?,
    })
}
