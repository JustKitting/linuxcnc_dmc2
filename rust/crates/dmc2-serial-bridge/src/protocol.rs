//! Parser and wire-level types for the Nano P3 protocol.

use dmc2_diagnostics::diagnostic_catalog;

pub const BOOT_MARKER: &str = "BOOT,P3,MYST1474-001,MONITOR_ONLY";
pub const MAX_SERIAL_LINE_BYTES: usize = 128;

diagnostic_catalog! {
    pub enum AxisCode: i32 {
        X = 0 => ("AXIS_X", "x", "the pendant axis selector requests the X axis", "no selector-specific operator action is required"),
        Y = 1 => ("AXIS_Y", "y", "the pendant axis selector requests the Y axis", "no selector-specific operator action is required"),
        Z = 2 => ("AXIS_Z", "z", "the pendant axis selector requests the Z axis", "no selector-specific operator action is required"),
        Axis4 = 3 => ("AXIS_4", "4", "the pendant axis selector requests the reserved fourth axis", "configure that axis before attempting to use its selector position"),
        Axis5 = 4 => ("AXIS_5", "5", "the pendant axis selector requests the reserved fifth axis", "configure that axis before attempting to use its selector position"),
        Off = -1 => ("AXIS_SELECTOR_OFF", "off", "the pendant axis selector is in its explicit off position", "select a configured axis before requesting a jog"),
        Invalid = -2 => ("AXIS_SELECTOR_INVALID", "invalid", "the pendant axis selector does not decode to one stable position", "place the selector in one stable labeled position and inspect its wiring if invalid persists")
    }
}

diagnostic_catalog! {
    pub enum MultiplierCode: i32 {
        X1 = 1 => ("MULTIPLIER_X1", "x1", "the pendant multiplier selector requests the base increment", "confirm the selected increment is appropriate before jogging"),
        X10 = 10 => ("MULTIPLIER_X10", "x10", "the pendant multiplier selector requests ten times the base increment", "confirm the selected increment is appropriate before jogging"),
        X100 = 100 => ("MULTIPLIER_X100", "x100", "the pendant multiplier selector requests one hundred times the base increment", "confirm the selected increment is appropriate before jogging"),
        Off = 0 => ("MULTIPLIER_SELECTOR_OFF", "off", "the pendant multiplier selector is in its explicit off position", "select a configured multiplier before requesting a jog"),
        Invalid = -1 => ("MULTIPLIER_SELECTOR_INVALID", "invalid", "the pendant multiplier selector does not decode to one stable position", "place the selector in one stable labeled position and inspect its wiring if invalid persists")
    }
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

diagnostic_catalog! {
    pub enum ProtocolError {
    WrongFieldCount = 1,
    "WRONG_FIELD_COUNT",
    "wrong-field-count",
    "the P3 packet did not contain exactly twelve comma-separated fields",
    "inspect the rejected Nano line and restore the exact P3 field layout";
    WrongMarker = 2,
    "WRONG_MARKER",
    "wrong-marker",
    "the packet marker was not the required P3 protocol marker",
    "verify the Nano is running the recorded P3 firmware and serial port";
    InvalidInteger = 3,
    "INVALID_INTEGER",
    "invalid-integer",
    "an integer field was noncanonical or outside its declared numeric range",
    "inspect the rejected line and the Nano formatter for the affected field";
    InvalidDetent = 4,
    "INVALID_DETENT",
    "invalid-detent",
    "latest-detent was outside the only accepted values -1, 0, and 1",
    "inspect the Nano quadrature decoder and rejected packet";
    InvalidAxis = 5,
    "INVALID_AXIS",
    "invalid-axis",
    "the axis selector token was outside the complete P3 axis vocabulary",
    "inspect selector wiring and the Nano axis encoder output";
    InvalidMultiplier = 6,
    "INVALID_MULTIPLIER",
    "invalid-multiplier",
    "the multiplier token was outside the complete P3 scale vocabulary",
    "inspect multiplier wiring and the Nano selector output";
    InvalidBoolean = 7,
    "INVALID_BOOLEAN",
    "invalid-boolean",
    "a P3 boolean field was neither canonical 0 nor canonical 1",
    "inspect the rejected line and Nano boolean formatting";
    RepeatedOrReversedSequence = 8,
    "REPEATED_OR_REVERSED_SEQUENCE",
    "repeated-or-reversed-sequence",
    "the packet sequence repeated or moved backward instead of advancing",
    "inspect retained previous/current sequence values and reset the Nano link";
    UnexpectedBootMarker = 9,
    "UNEXPECTED_BOOT_MARKER",
    "unexpected-boot-marker",
    "the Nano emitted a BOOT record that did not exactly match the accepted firmware identity",
    "verify the connected pendant firmware and exact boot marker";
    NonAscii = 10,
    "NON_ASCII",
    "non-ascii",
    "the serial frame contained bytes outside the ASCII P3 protocol",
    "inspect retained line length, serial settings, grounding, and Nano output";
    OverlongLine = 11,
    "OVERLONG_LINE",
    "overlong-line",
    "the serial frame exceeded the bounded 128-byte P3 line length",
    "inspect firmware framing and serial corruption before reconnecting";
    }
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
