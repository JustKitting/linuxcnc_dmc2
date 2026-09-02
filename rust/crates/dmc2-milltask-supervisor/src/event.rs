use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub const SCHEMA: &str = "dmc2-milltask-lifecycle-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    kind: &'static str,
    unix_ns: u128,
    supervisor_pid: u32,
    fields: Vec<(&'static str, String)>,
}

impl Event {
    pub fn new(kind: &'static str, unix_ns: u128, supervisor_pid: u32) -> Self {
        Self {
            kind,
            unix_ns,
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

    pub fn render(&self) -> String {
        let mut rendered = format!(
            "schema={SCHEMA}\tevent={}\tunix_ns={}\tsupervisor_pid={}",
            self.kind, self.unix_ns, self.supervisor_pid
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_only_tab_safe_dynamic_values() {
        let event = Event::new("started", 42, 7)
            .encoded_os_field("program_hex", OsStr::new("a\tb\nc"))
            .field("child_pid", 9);

        assert_eq!(
            event.render(),
            "schema=dmc2-milltask-lifecycle-v1\tevent=started\tunix_ns=42\tsupervisor_pid=7\tprogram_hex=6109620a63\tchild_pid=9"
        );
    }
}
