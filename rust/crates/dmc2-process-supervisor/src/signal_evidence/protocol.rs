use std::io;

use crate::event::{hex_bytes, signal_name, Event};

pub const RECORD_BYTES: usize = 64;
pub const RECORD_VERSION: u32 = 1;
const RECORD_MAGIC: &[u8; 8] = b"DMC2SIG1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    pub kind: RecordKind,
    pub signal_number: i32,
    signal_code: i32,
    signal_errno: i32,
    pub target_pid: u32,
    target_tid: u32,
    sender_pid: u32,
    sender_uid: u32,
    realtime_seconds: i64,
    realtime_nanoseconds: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    Initialized,
    HandlerArmed,
    SignalDelivered,
    HandlerDisarmed,
}

#[derive(Debug)]
pub enum Observation {
    Record {
        record: Record,
        target_matches: bool,
    },
    InvalidRecord {
        reason: &'static str,
        raw: [u8; RECORD_BYTES],
    },
    ReadFailed {
        kind: io::ErrorKind,
        raw_os_error: Option<i32>,
        detail: String,
    },
    MalformedPacket {
        observed_bytes: usize,
        raw: Vec<u8>,
    },
    ChannelClosed,
}

pub fn parse_record(raw: [u8; RECORD_BYTES]) -> Result<Record, &'static str> {
    if !raw.starts_with(RECORD_MAGIC) {
        return Err("magic-mismatch");
    }
    if read_u32(&raw, 8)? != RECORD_VERSION {
        return Err("version-mismatch");
    }
    if read_u32(&raw, 12)? != RECORD_BYTES as u32 {
        return Err("record-size-mismatch");
    }
    let kind = match read_u32(&raw, 16)? {
        1 => RecordKind::Initialized,
        2 => RecordKind::HandlerArmed,
        3 => RecordKind::SignalDelivered,
        4 => RecordKind::HandlerDisarmed,
        _ => return Err("unknown-record-kind"),
    };
    let signal_number = read_i32(&raw, 20)?;
    match kind {
        RecordKind::Initialized if signal_number != 0 => {
            return Err("initialized-record-has-signal");
        }
        RecordKind::HandlerArmed | RecordKind::SignalDelivered | RecordKind::HandlerDisarmed
            if !matches!(signal_number, 2 | 15) =>
        {
            return Err("record-signal-outside-contract");
        }
        _ => {}
    }
    let target_pid = read_u32(&raw, 32)?;
    let target_tid = read_u32(&raw, 36)?;
    if target_pid == 0 || target_tid == 0 {
        return Err("zero-target-identity");
    }
    let realtime_seconds = read_i64(&raw, 48)?;
    let realtime_nanoseconds = read_i64(&raw, 56)?;
    if realtime_seconds < 0 || !(0..1_000_000_000).contains(&realtime_nanoseconds) {
        return Err("invalid-realtime-timestamp");
    }
    Ok(Record {
        kind,
        signal_number,
        signal_code: read_i32(&raw, 24)?,
        signal_errno: read_i32(&raw, 28)?,
        target_pid,
        target_tid,
        sender_pid: read_u32(&raw, 40)?,
        sender_uid: read_u32(&raw, 44)?,
        realtime_seconds,
        realtime_nanoseconds,
    })
}

impl Observation {
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::Record { .. } => "caught-signal-evidence-record",
            Self::InvalidRecord { .. } => "caught-signal-evidence-invalid-record",
            Self::ReadFailed { .. } => "caught-signal-evidence-read-failed",
            Self::MalformedPacket { .. } => "caught-signal-evidence-malformed-packet",
            Self::ChannelClosed => "caught-signal-evidence-channel-closed",
        }
    }

    pub fn event_fields(&self, event: Event) -> Event {
        match self {
            Self::Record {
                record,
                target_matches,
            } => record
                .event_fields(event)
                .field("caught_signal_target_matches", target_matches),
            Self::InvalidRecord { reason, raw } => event
                .field("caught_signal_invalid_reason", reason)
                .field("caught_signal_invalid_raw_hex", hex_bytes(raw)),
            Self::ReadFailed {
                kind,
                raw_os_error,
                detail,
            } => event
                .field("caught_signal_read_error_kind", format!("{kind:?}"))
                .field(
                    "caught_signal_read_raw_os_error",
                    optional_i32(*raw_os_error),
                )
                .field("caught_signal_read_error_hex", hex_bytes(detail.as_bytes())),
            Self::MalformedPacket {
                observed_bytes,
                raw,
            } => event
                .field("caught_signal_packet_bytes", observed_bytes)
                .field("caught_signal_packet_raw_hex", hex_bytes(raw)),
            Self::ChannelClosed => event.field("caught_signal_channel_close_state", "peer-closed"),
        }
    }
}

impl Record {
    fn event_fields(self, event: Event) -> Event {
        event
            .field("caught_signal_record_kind", self.kind.name())
            .field("caught_signal_record_kind_code", self.kind.code())
            .field("caught_signal", self.signal_number)
            .field(
                "caught_signal_name",
                signal_name_or_none(self.signal_number),
            )
            .field("caught_signal_code", self.signal_code)
            .field(
                "caught_signal_code_name",
                signal_code_name(self.signal_code),
            )
            .field("caught_signal_errno", self.signal_errno)
            .field("caught_signal_target_pid", self.target_pid)
            .field("caught_signal_target_tid", self.target_tid)
            .field("caught_signal_sender_pid", self.sender_pid)
            .field("caught_signal_sender_uid", self.sender_uid)
            .field(
                "caught_signal_sender_identity_valid",
                sender_identity_valid(self.signal_code),
            )
            .field("caught_signal_realtime_seconds", self.realtime_seconds)
            .field(
                "caught_signal_realtime_nanoseconds",
                self.realtime_nanoseconds,
            )
    }

    pub fn summary_fields(self, event: Event) -> Event {
        event
            .field("last_caught_signal", self.signal_number)
            .field("last_caught_signal_name", signal_name(self.signal_number))
            .field("last_caught_signal_code", self.signal_code)
            .field(
                "last_caught_signal_code_name",
                signal_code_name(self.signal_code),
            )
            .field("last_caught_signal_sender_pid", self.sender_pid)
            .field("last_caught_signal_sender_uid", self.sender_uid)
            .field(
                "last_caught_signal_sender_identity_valid",
                sender_identity_valid(self.signal_code),
            )
            .field("last_caught_signal_target_tid", self.target_tid)
            .field("last_caught_signal_realtime_seconds", self.realtime_seconds)
            .field(
                "last_caught_signal_realtime_nanoseconds",
                self.realtime_nanoseconds,
            )
    }
}

impl RecordKind {
    fn name(self) -> &'static str {
        match self {
            Self::Initialized => "initialized",
            Self::HandlerArmed => "handler-armed",
            Self::SignalDelivered => "signal-delivered",
            Self::HandlerDisarmed => "handler-disarmed",
        }
    }

    fn code(self) -> u32 {
        match self {
            Self::Initialized => 1,
            Self::HandlerArmed => 2,
            Self::SignalDelivered => 3,
            Self::HandlerDisarmed => 4,
        }
    }
}

fn signal_code_name(code: i32) -> &'static str {
    match code {
        0 => "SI_USER",
        -1 => "SI_QUEUE",
        -2 => "SI_TIMER",
        -3 => "SI_MESGQ",
        -4 => "SI_ASYNCIO",
        -5 => "SI_SIGIO",
        -6 => "SI_TKILL",
        -7 => "SI_DETHREAD",
        128 => "SI_KERNEL",
        _ => "UNKNOWN_SI_CODE",
    }
}

fn sender_identity_valid(code: i32) -> bool {
    matches!(code, 0 | -1 | -6)
}

fn signal_name_or_none(signal: i32) -> &'static str {
    if signal == 0 {
        "NONE"
    } else {
        signal_name(signal)
    }
}

fn read_u32(raw: &[u8; RECORD_BYTES], offset: usize) -> Result<u32, &'static str> {
    let bytes = raw
        .get(offset..offset.saturating_add(4))
        .ok_or("u32-field-out-of-range")?;
    let mut value = [0_u8; 4];
    value.copy_from_slice(bytes);
    Ok(u32::from_ne_bytes(value))
}

fn read_i32(raw: &[u8; RECORD_BYTES], offset: usize) -> Result<i32, &'static str> {
    let bytes = raw
        .get(offset..offset.saturating_add(4))
        .ok_or("i32-field-out-of-range")?;
    let mut value = [0_u8; 4];
    value.copy_from_slice(bytes);
    Ok(i32::from_ne_bytes(value))
}

fn read_i64(raw: &[u8; RECORD_BYTES], offset: usize) -> Result<i64, &'static str> {
    let bytes = raw
        .get(offset..offset.saturating_add(8))
        .ok_or("i64-field-out-of-range")?;
    let mut value = [0_u8; 8];
    value.copy_from_slice(bytes);
    Ok(i64::from_ne_bytes(value))
}

fn optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "NONE".to_owned(), |item| item.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: u32, signal: i32, pid: u32) -> [u8; RECORD_BYTES] {
        let mut raw = [0_u8; RECORD_BYTES];
        raw[..8].copy_from_slice(RECORD_MAGIC);
        raw[8..12].copy_from_slice(&RECORD_VERSION.to_ne_bytes());
        raw[12..16].copy_from_slice(&(RECORD_BYTES as u32).to_ne_bytes());
        raw[16..20].copy_from_slice(&kind.to_ne_bytes());
        raw[20..24].copy_from_slice(&signal.to_ne_bytes());
        raw[32..36].copy_from_slice(&pid.to_ne_bytes());
        raw[36..40].copy_from_slice(&pid.to_ne_bytes());
        raw
    }

    #[test]
    fn parses_the_fixed_native_protocol() {
        let parsed = parse_record(record(2, 15, 42)).expect("valid protocol record");

        assert_eq!(parsed.kind, RecordKind::HandlerArmed);
        assert_eq!(parsed.signal_number, 15);
        assert_eq!(parsed.target_pid, 42);
    }

    #[test]
    fn rejects_an_unknown_signal_before_it_can_affect_state() {
        assert_eq!(
            parse_record(record(3, 9, 42)),
            Err("record-signal-outside-contract")
        );
    }
}
