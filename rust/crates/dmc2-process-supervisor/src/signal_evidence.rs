mod channel;
mod elf;
mod protocol;

use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::catalog::{CaughtSignalEvidence, Criticality, ProcessRole};
use crate::event::Event;

use self::channel::{evidence_channel, ChannelError, SendBufferEvidence};
use self::elf::{inspect_library, LibraryError, LibraryEvidence};
use self::protocol::{parse_record, Observation, Record, RecordKind, RECORD_BYTES, RECORD_VERSION};

const LD_PRELOAD_ENV: &str = "LD_PRELOAD";
const FD_ENV: &str = "DMC2_SIGNAL_EVIDENCE_FD";
const PROTOCOL_ENV: &str = "DMC2_SIGNAL_EVIDENCE_PROTOCOL";
const TEST_LIBRARY_ENV: &str = "DMC2_SIGNAL_EVIDENCE_TEST_LIBRARY";
const LIBRARY_NAME: &str = "libdmc2_signal_evidence.so";

pub struct Tracker {
    contract: CaughtSignalEvidence,
    state: State,
}

enum State {
    Disabled,
    Enabled(Enabled),
}

struct Enabled {
    reader: File,
    child_writer: Option<std::os::fd::OwnedFd>,
    library: LibraryEvidence,
    channel_send_buffer: SendBufferEvidence,
    initialized_records: u64,
    current_armed_mask: u8,
    ever_armed_mask: u8,
    records: u64,
    target_records: u64,
    delivered: u64,
    matching_delivered: u64,
    foreign_target_records: u64,
    invalid_records: u64,
    read_failures: u64,
    malformed_packets: u64,
    eof: bool,
    eof_reported: bool,
    reader_failed: bool,
    last_matching_delivery: Option<Record>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InfrastructureFailures {
    pub invalid_records: bool,
    pub read_failures: bool,
    pub malformed_packets: bool,
    pub initialization_record_count_invalid: bool,
    pub handler_registration_incomplete: bool,
    pub channel_not_closed: bool,
}

impl InfrastructureFailures {
    fn any(self) -> bool {
        self.invalid_records
            || self.read_failures
            || self.malformed_packets
            || self.initialization_record_count_invalid
            || self.handler_registration_incomplete
            || self.channel_not_closed
    }
}

#[derive(Debug)]
pub enum Error {
    TestOverrideForProduction { role: &'static str, path: OsString },
    InheritedPreload(OsString),
    CurrentExecutable(io::Error),
    MissingExecutableDirectory(PathBuf),
    Library(LibraryError),
    Channel(ChannelError),
    ChildWriterUnavailable,
}

impl Tracker {
    pub fn prepare(role: ProcessRole) -> Result<Self, Error> {
        let contract = role.caught_signal_evidence();
        if contract == CaughtSignalEvidence::None {
            return Ok(Self {
                contract,
                state: State::Disabled,
            });
        }
        if let Some(value) = std::env::var_os(LD_PRELOAD_ENV) {
            return Err(Error::InheritedPreload(value));
        }

        let library_path = library_path(role)?;
        let library =
            inspect_library(&library_path, Path::new(role.program())).map_err(Error::Library)?;
        let channel = evidence_channel().map_err(Error::Channel)?;
        Ok(Self {
            contract,
            state: State::Enabled(Enabled {
                reader: channel.reader,
                child_writer: Some(channel.writer),
                library,
                channel_send_buffer: channel.send_buffer,
                initialized_records: 0,
                current_armed_mask: 0,
                ever_armed_mask: 0,
                records: 0,
                target_records: 0,
                delivered: 0,
                matching_delivered: 0,
                foreign_target_records: 0,
                invalid_records: 0,
                read_failures: 0,
                malformed_packets: 0,
                eof: false,
                eof_reported: false,
                reader_failed: false,
                last_matching_delivery: None,
            }),
        })
    }

    pub fn configure_child(&self, command: &mut Command) -> Result<(), Error> {
        let State::Enabled(enabled) = &self.state else {
            return Ok(());
        };
        let writer = enabled
            .child_writer
            .as_ref()
            .ok_or(Error::ChildWriterUnavailable)?;
        command.env(LD_PRELOAD_ENV, &enabled.library.path);
        command.env(FD_ENV, writer.as_raw_fd().to_string());
        command.env(PROTOCOL_ENV, RECORD_VERSION.to_string());
        command.env_remove(TEST_LIBRARY_ENV);
        Ok(())
    }

    pub fn parent_after_spawn(&mut self) {
        if let State::Enabled(enabled) = &mut self.state {
            enabled.child_writer.take();
        }
    }

    pub fn drain(&mut self, target_pid: u32) -> Vec<Observation> {
        let State::Enabled(enabled) = &mut self.state else {
            return Vec::new();
        };
        if enabled.eof || enabled.reader_failed {
            return Vec::new();
        }

        let mut observations = Vec::new();
        let mut buffer = [0_u8; RECORD_BYTES + 1];
        loop {
            match enabled.reader.read(&mut buffer) {
                Ok(0) => {
                    enabled.eof = true;
                    if !enabled.eof_reported {
                        enabled.eof_reported = true;
                        observations.push(Observation::ChannelClosed);
                    }
                    break;
                }
                Ok(bytes) => {
                    if bytes != RECORD_BYTES {
                        enabled.malformed_packets = enabled.malformed_packets.saturating_add(1);
                        observations.push(Observation::MalformedPacket {
                            observed_bytes: bytes,
                            raw: buffer[..bytes].to_vec(),
                        });
                        continue;
                    }
                    let mut raw = [0_u8; RECORD_BYTES];
                    raw.copy_from_slice(&buffer[..RECORD_BYTES]);
                    match parse_record(raw) {
                        Ok(record) => {
                            let target_matches = record.target_pid == target_pid;
                            enabled.apply_record(record, target_matches);
                            observations.push(Observation::Record {
                                record,
                                target_matches,
                            });
                        }
                        Err(reason) => {
                            enabled.invalid_records = enabled.invalid_records.saturating_add(1);
                            observations.push(Observation::InvalidRecord { reason, raw });
                        }
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    enabled.read_failures = enabled.read_failures.saturating_add(1);
                    enabled.reader_failed = true;
                    observations.push(Observation::ReadFailed {
                        kind: error.kind(),
                        raw_os_error: error.raw_os_error(),
                        detail: error.to_string(),
                    });
                    break;
                }
            }
        }
        observations
    }

    pub fn plan_event_fields(&self, event: Event) -> Event {
        let event = event
            .field(
                "caught_signal_expected_signals",
                expected_signals(self.contract),
            )
            .field("caught_signal_record_version", RECORD_VERSION)
            .field("caught_signal_record_bytes", RECORD_BYTES);
        match &self.state {
            State::Disabled => event.field("caught_signal_evidence_plan", "disabled"),
            State::Enabled(enabled) => {
                let event = enabled
                    .library
                    .event_fields(event.field("caught_signal_evidence_plan", "configured"))
                    .field("caught_signal_transport", "unix-seqpacket-msg-nosignal")
                    .field("caught_signal_channel_validation", "send-receive-loopback")
                    .field(
                        "caught_signal_delivery_record_semantics",
                        "best-effort-nonblocking",
                    )
                    .field("caught_signal_delivery_drop_counter_available", false);
                enabled.channel_send_buffer.event_fields(event)
            }
        }
    }

    pub fn summary_event_fields(&self, event: Event) -> Event {
        let State::Enabled(enabled) = &self.state else {
            return event.field("caught_signal_evidence_state", "not-applicable");
        };
        let event = event
            .field(
                "caught_signal_evidence_state",
                enabled.evidence_state(self.contract),
            )
            .field(
                "caught_signal_delivery_record_semantics",
                "best-effort-nonblocking",
            )
            .field("caught_signal_delivery_drop_counter_available", false)
            .field("caught_signal_initialized", enabled.initialized_records > 0)
            .field(
                "caught_signal_initialized_records",
                enabled.initialized_records,
            )
            .field("caught_signal_records", enabled.records)
            .field("caught_signal_target_records", enabled.target_records)
            .field(
                "caught_signal_foreign_target_records",
                enabled.foreign_target_records,
            )
            .field("caught_signal_delivered_records", enabled.delivered)
            .field(
                "caught_signal_matching_delivered_records",
                enabled.matching_delivered,
            )
            .field("caught_signal_invalid_records", enabled.invalid_records)
            .field("caught_signal_read_failures", enabled.read_failures)
            .field("caught_signal_malformed_packets", enabled.malformed_packets)
            .field("caught_signal_channel_eof", enabled.eof)
            .field("caught_signal_reader_failed", enabled.reader_failed)
            .field(
                "caught_signal_current_armed_signals",
                render_signal_mask(enabled.current_armed_mask),
            )
            .field(
                "caught_signal_ever_armed_signals",
                render_signal_mask(enabled.ever_armed_mask),
            )
            .field(
                "caught_signal_expected_handlers_observed",
                enabled.expected_handlers_observed(self.contract),
            )
            .field(
                "caught_signal_expected_handlers_active_at_exit",
                enabled.expected_handlers_active(self.contract),
            );
        match enabled.last_matching_delivery {
            Some(record) => record.summary_fields(event),
            None => event
                .field("last_caught_signal", "NONE")
                .field("last_caught_signal_name", "NONE")
                .field("last_caught_signal_code", "NONE")
                .field("last_caught_signal_code_name", "NONE")
                .field("last_caught_signal_sender_pid", "NONE")
                .field("last_caught_signal_sender_uid", "NONE")
                .field("last_caught_signal_sender_identity_valid", false)
                .field("last_caught_signal_target_tid", "NONE")
                .field("last_caught_signal_realtime_seconds", "NONE")
                .field("last_caught_signal_realtime_nanoseconds", "NONE"),
        }
    }

    pub fn zero_exit_outcome(&self) -> &'static str {
        let State::Enabled(enabled) = &self.state else {
            return "zero-exit";
        };
        if enabled.matching_delivered > 0 && enabled.evidence_state(self.contract) == "ready" {
            "caught-signal-clean-exit"
        } else if enabled.matching_delivered > 0 {
            "caught-signal-observed-zero-exit-with-incomplete-evidence"
        } else if enabled.evidence_state(self.contract) == "ready" {
            "zero-exit-without-caught-signal-evidence"
        } else {
            "zero-exit-with-incomplete-caught-signal-evidence"
        }
    }

    pub fn infrastructure_failures(&self) -> Option<InfrastructureFailures> {
        let State::Enabled(enabled) = &self.state else {
            return None;
        };
        let failures = InfrastructureFailures {
            invalid_records: enabled.invalid_records > 0,
            read_failures: enabled.read_failures > 0,
            malformed_packets: enabled.malformed_packets > 0,
            initialization_record_count_invalid: enabled.initialized_records != 1,
            handler_registration_incomplete: !enabled.expected_handlers_observed(self.contract)
                || !enabled.expected_handlers_active(self.contract),
            channel_not_closed: !enabled.eof,
        };
        failures.any().then_some(failures)
    }
}

impl Enabled {
    fn apply_record(&mut self, record: Record, target_matches: bool) {
        self.records = self.records.saturating_add(1);
        if !target_matches {
            self.foreign_target_records = self.foreign_target_records.saturating_add(1);
            return;
        }
        self.target_records = self.target_records.saturating_add(1);
        match record.kind {
            RecordKind::Initialized => {
                self.initialized_records = self.initialized_records.saturating_add(1);
            }
            RecordKind::HandlerArmed => {
                if let Some(mask) = signal_mask(record.signal_number) {
                    self.current_armed_mask |= mask;
                    self.ever_armed_mask |= mask;
                }
            }
            RecordKind::HandlerDisarmed => {
                if let Some(mask) = signal_mask(record.signal_number) {
                    self.current_armed_mask &= !mask;
                }
            }
            RecordKind::SignalDelivered => {
                self.delivered = self.delivered.saturating_add(1);
                if matches!(record.signal_number, 2 | 15) {
                    self.matching_delivered = self.matching_delivered.saturating_add(1);
                    self.last_matching_delivery = Some(record);
                }
            }
        }
    }

    fn expected_handlers_observed(&self, contract: CaughtSignalEvidence) -> bool {
        contract
            .expected_signals()
            .iter()
            .all(|signal| signal_mask(*signal).is_some_and(|mask| self.ever_armed_mask & mask != 0))
    }

    fn expected_handlers_active(&self, contract: CaughtSignalEvidence) -> bool {
        contract.expected_signals().iter().all(|signal| {
            signal_mask(*signal).is_some_and(|mask| self.current_armed_mask & mask != 0)
        })
    }

    fn evidence_state(&self, contract: CaughtSignalEvidence) -> &'static str {
        if self.invalid_records > 0 || self.read_failures > 0 || self.malformed_packets > 0 {
            "channel-compromised"
        } else if self.initialized_records == 0 {
            "initialization-not-observed"
        } else if self.initialized_records != 1 {
            "initialization-record-count-invalid"
        } else if !self.expected_handlers_observed(contract)
            || !self.expected_handlers_active(contract)
        {
            "handler-registration-incomplete"
        } else {
            "ready"
        }
    }
}

fn library_path(role: ProcessRole) -> Result<PathBuf, Error> {
    if let Some(path) = std::env::var_os(TEST_LIBRARY_ENV) {
        if role.criticality() == Criticality::VerificationOnly {
            return Ok(PathBuf::from(path));
        }
        return Err(Error::TestOverrideForProduction {
            role: role.name(),
            path,
        });
    }
    let executable = std::env::current_exe().map_err(Error::CurrentExecutable)?;
    let directory = executable
        .parent()
        .ok_or_else(|| Error::MissingExecutableDirectory(executable.clone()))?;
    Ok(directory.join(LIBRARY_NAME))
}

fn expected_signals(contract: CaughtSignalEvidence) -> String {
    contract
        .expected_signals()
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn signal_mask(signal: i32) -> Option<u8> {
    match signal {
        2 => Some(1),
        15 => Some(2),
        _ => None,
    }
}

fn render_signal_mask(mask: u8) -> String {
    [2, 15]
        .into_iter()
        .filter(|signal| signal_mask(*signal).is_some_and(|bit| mask & bit != 0))
        .map(|signal| signal.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

impl fmt::Display for InfrastructureFailures {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid_records={}, read_failures={}, malformed_packets={}, initialization_record_count_invalid={}, handler_registration_incomplete={}, channel_not_closed={}",
            self.invalid_records,
            self.read_failures,
            self.malformed_packets,
            self.initialization_record_count_invalid,
            self.handler_registration_incomplete,
            self.channel_not_closed,
        )
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TestOverrideForProduction { role, path } => write!(
                formatter,
                "test signal-evidence library override {path:?} is forbidden for production role {role}"
            ),
            Self::InheritedPreload(value) => write!(
                formatter,
                "refusing to replace inherited LD_PRELOAD value {value:?}"
            ),
            Self::CurrentExecutable(error) => {
                write!(formatter, "resolve current executable for signal evidence: {error}")
            }
            Self::MissingExecutableDirectory(path) => write!(
                formatter,
                "signal-evidence owner executable has no parent directory: {}",
                path.display()
            ),
            Self::Library(error) => write!(formatter, "signal-evidence library invalid: {error}"),
            Self::Channel(error) => {
                write!(formatter, "signal-evidence channel unavailable: {error}")
            }
            Self::ChildWriterUnavailable => write!(
                formatter,
                "signal-evidence child writer was unavailable before process spawn"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CurrentExecutable(error) => Some(error),
            Self::Library(error) => Some(error),
            Self::Channel(error) => Some(error),
            Self::TestOverrideForProduction { .. }
            | Self::InheritedPreload(_)
            | Self::MissingExecutableDirectory(_)
            | Self::ChildWriterUnavailable => None,
        }
    }
}

pub fn setup_error_event_fields(error: &Error, event: Event) -> Event {
    event
        .field("caught_signal_setup_error", error_kind(error))
        .field(
            "caught_signal_setup_error_hex",
            crate::event::hex_bytes(error.to_string().as_bytes()),
        )
}

fn error_kind(error: &Error) -> &'static str {
    match error {
        Error::TestOverrideForProduction { .. } => "test-override-for-production",
        Error::InheritedPreload(_) => "inherited-preload",
        Error::CurrentExecutable(_) => "current-executable",
        Error::MissingExecutableDirectory(_) => "missing-executable-directory",
        Error::Library(_) => "library-validation",
        Error::Channel(_) => "channel-setup",
        Error::ChildWriterUnavailable => "child-writer-unavailable",
    }
}

pub fn observation_event(observation: &Observation, event: Event) -> Event {
    observation.event_fields(event)
}

pub fn observation_event_name(observation: &Observation) -> &'static str {
    observation.event_name()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_rendering_is_stable_and_ordered() {
        assert_eq!(render_signal_mask(0), "");
        assert_eq!(render_signal_mask(1), "2");
        assert_eq!(render_signal_mask(2), "15");
        assert_eq!(render_signal_mask(3), "2,15");
    }

    #[test]
    fn disabled_tracker_does_not_modify_child_environment() {
        let role = crate::catalog::role(std::ffi::OsStr::new("lifecycle-test"))
            .expect("verification role");
        let tracker = Tracker::prepare(role).expect("disabled signal tracker");
        let mut command = Command::new("/bin/true");
        tracker
            .configure_child(&mut command)
            .expect("configure disabled tracker");

        assert!(tracker.infrastructure_failures().is_none());
        assert_eq!(tracker.zero_exit_outcome(), "zero-exit");
    }
}
