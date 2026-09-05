use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::SystemTimeError;

use dmc2_diagnostics::RecoveryClassified;

pub const SCHEMA: &str = "dmc2-process-lifecycle-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventTime {
    Captured(u128),
    BeforeUnixEpoch { by_ns: u128 },
}

impl From<u128> for EventTime {
    fn from(value: u128) -> Self {
        Self::Captured(value)
    }
}

impl From<Result<u128, SystemTimeError>> for EventTime {
    fn from(value: Result<u128, SystemTimeError>) -> Self {
        match value {
            Ok(value) => Self::Captured(value),
            Err(error) => Self::BeforeUnixEpoch {
                by_ns: error.duration().as_nanos(),
            },
        }
    }
}

impl fmt::Display for EventTime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Captured(value) => value.fmt(formatter),
            Self::BeforeUnixEpoch { by_ns } => {
                write!(formatter, "ERROR_CLOCK_BEFORE_UNIX_EPOCH_BY_{by_ns}_NS")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    kind: &'static str,
    unix_ns: EventTime,
    supervisor_pid: u32,
    fields: Vec<(&'static str, String)>,
}

impl Event {
    pub fn new(kind: &'static str, unix_ns: impl Into<EventTime>, supervisor_pid: u32) -> Self {
        Self {
            kind,
            unix_ns: unix_ns.into(),
            supervisor_pid,
            fields: Vec::new(),
        }
    }

    pub fn field(mut self, name: &'static str, value: impl ToString) -> Self {
        self.fields.push((name, value.to_string()));
        self
    }

    pub fn encoded_os_field(mut self, name: &'static str, value: &OsStr) -> Self {
        self.fields.push((name, hex_bytes(value.as_bytes())));
        self
    }

    pub fn encoded_path_field(self, name: &'static str, value: &Path) -> Self {
        self.encoded_os_field(name, value.as_os_str())
    }

    pub fn recovery(mut self, issue: &impl RecoveryClassified) -> Self {
        let recovery = issue.recovery_class();
        let operations = recovery
            .ui_operations()
            .iter()
            .map(|operation| operation.id())
            .collect::<Vec<_>>()
            .join(";");
        self.fields
            .push(("recovery_class", recovery.name().to_owned()));
        self.fields.push((
            "recovery_transition",
            recovery.transition().name().to_owned(),
        ));
        self.fields.push((
            "recovery_clear_condition",
            recovery.clear_transition().to_owned(),
        ));
        self.fields.push(("recovery_ui_operations", operations));
        self
    }

    pub fn render(&self) -> String {
        let mut rendered = format!(
            "schema={SCHEMA}\tevent={}\tunix_ns={}\tsupervisor_pid={}",
            self.kind, self.unix_ns, self.supervisor_pid
        );
        if let EventTime::BeforeUnixEpoch { by_ns } = self.unix_ns {
            rendered.push_str(&format!(
                "\tclock_state=error\tclock_error=system clock precedes the Unix epoch by {by_ns} ns; timestamp unavailable; correct system time and relaunch through Applications"
            ));
        }
        for (name, value) in &self.fields {
            rendered.push('\t');
            rendered.push_str(name);
            rendered.push('=');
            rendered.push_str(value);
        }
        rendered
    }
}

pub fn encode_arguments(arguments: &[OsString]) -> String {
    arguments
        .iter()
        .map(|argument| hex_bytes(argument.as_bytes()))
        .collect::<Vec<_>>()
        .join(",")
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

pub fn signal_name(signal: i32) -> &'static str {
    match signal {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        10 => "SIGUSR1",
        11 => "SIGSEGV",
        12 => "SIGUSR2",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        16 => "SIGSTKFLT",
        17 => "SIGCHLD",
        18 => "SIGCONT",
        19 => "SIGSTOP",
        20 => "SIGTSTP",
        21 => "SIGTTIN",
        22 => "SIGTTOU",
        23 => "SIGURG",
        24 => "SIGXCPU",
        25 => "SIGXFSZ",
        26 => "SIGVTALRM",
        27 => "SIGPROF",
        28 => "SIGWINCH",
        29 => "SIGIO",
        30 => "SIGPWR",
        31 => "SIGSYS",
        _ => "UNKNOWN_SIGNAL",
    }
}
