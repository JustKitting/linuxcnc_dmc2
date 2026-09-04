use std::io;
use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_long};
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

use crate::event::{signal_name, Event};

const WNOHANG: c_int = 1;
const ECHILD_LINUX: i32 = 10;

#[derive(Debug, Clone, Copy)]
pub struct WaitEvidence {
    pub pid: u32,
    pub status: ExitStatus,
    usage: ResourceUsage,
}

impl WaitEvidence {
    pub fn event_fields(self, event: Event) -> Event {
        event
            .field("child_pid", self.pid)
            .field("raw_wait_status", self.status.into_raw())
            .field("exit_code", optional_i32(self.status.code()))
            .field("signal", optional_i32(self.status.signal()))
            .field(
                "signal_name",
                self.status.signal().map(signal_name).unwrap_or("NONE"),
            )
            .field("core_dumped", self.status.core_dumped())
            .field("rusage_user_seconds", self.usage.user_seconds)
            .field("rusage_user_microseconds", self.usage.user_microseconds)
            .field("rusage_system_seconds", self.usage.system_seconds)
            .field("rusage_system_microseconds", self.usage.system_microseconds)
            .field("rusage_max_resident_kib", self.usage.max_resident_kib)
            .field("rusage_minor_faults", self.usage.minor_faults)
            .field("rusage_major_faults", self.usage.major_faults)
            .field("rusage_block_reads", self.usage.block_reads)
            .field("rusage_block_writes", self.usage.block_writes)
            .field("rusage_signals_delivered", self.usage.signals_delivered)
            .field(
                "rusage_voluntary_context_switches",
                self.usage.voluntary_context_switches,
            )
            .field(
                "rusage_involuntary_context_switches",
                self.usage.involuntary_context_switches,
            )
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Poll {
    Running,
    Terminal(WaitEvidence),
    NoChildren,
}

pub fn wait_pid(pid: u32) -> io::Result<WaitEvidence> {
    wait4_call(pid_to_raw(pid)?, 0)?.ok_or_else(|| {
        io::Error::other(format!(
            "blocking wait4 returned no terminal status for child PID {pid}"
        ))
    })
}

pub fn poll_any() -> io::Result<Poll> {
    match wait4_call(-1, WNOHANG) {
        Ok(Some(evidence)) => Ok(Poll::Terminal(evidence)),
        Ok(None) => Ok(Poll::Running),
        Err(error) if error.raw_os_error() == Some(ECHILD_LINUX) => Ok(Poll::NoChildren),
        Err(error) => Err(error),
    }
}

fn pid_to_raw(pid: u32) -> io::Result<c_int> {
    c_int::try_from(pid).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("child PID {pid} does not fit pid_t"),
        )
    })
}

fn wait4_call(target: c_int, options: c_int) -> io::Result<Option<WaitEvidence>> {
    loop {
        let mut status = 0;
        let mut usage = MaybeUninit::<RawResourceUsage>::zeroed();
        // SAFETY: status and usage are writable for the syscall duration; wait4 retains neither pointer.
        let result = unsafe { wait4(target, &mut status, options, usage.as_mut_ptr()) };
        if result > 0 {
            if target != -1 && result != target {
                return Err(io::Error::other(format!(
                    "wait4 returned PID {result} while waiting for {target}"
                )));
            }
            // SAFETY: successful wait4 initialized the resource-usage object.
            let usage = unsafe { usage.assume_init() };
            return Ok(Some(WaitEvidence {
                pid: u32::try_from(result)
                    .map_err(|_| io::Error::other("wait4 returned an invalid positive PID"))?,
                status: ExitStatus::from_raw(status),
                usage: usage.into(),
            }));
        }
        if result == 0 && options & WNOHANG != 0 {
            return Ok(None);
        }
        if result >= 0 {
            return Err(io::Error::other(format!(
                "wait4 returned unexpected value {result} for target {target}"
            )));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ResourceUsage {
    user_seconds: c_long,
    user_microseconds: c_long,
    system_seconds: c_long,
    system_microseconds: c_long,
    max_resident_kib: c_long,
    minor_faults: c_long,
    major_faults: c_long,
    block_reads: c_long,
    block_writes: c_long,
    signals_delivered: c_long,
    voluntary_context_switches: c_long,
    involuntary_context_switches: c_long,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TimeVal {
    seconds: c_long,
    microseconds: c_long,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawResourceUsage {
    user: TimeVal,
    system: TimeVal,
    max_resident_kib: c_long,
    shared_text_raw: c_long,
    unshared_data_raw: c_long,
    unshared_stack_raw: c_long,
    minor_faults: c_long,
    major_faults: c_long,
    swaps: c_long,
    block_reads: c_long,
    block_writes: c_long,
    ipc_messages_sent: c_long,
    ipc_messages_received: c_long,
    signals_delivered: c_long,
    voluntary_context_switches: c_long,
    involuntary_context_switches: c_long,
}

impl From<RawResourceUsage> for ResourceUsage {
    fn from(value: RawResourceUsage) -> Self {
        Self {
            user_seconds: value.user.seconds,
            user_microseconds: value.user.microseconds,
            system_seconds: value.system.seconds,
            system_microseconds: value.system.microseconds,
            max_resident_kib: value.max_resident_kib,
            minor_faults: value.minor_faults,
            major_faults: value.major_faults,
            block_reads: value.block_reads,
            block_writes: value.block_writes,
            signals_delivered: value.signals_delivered,
            voluntary_context_switches: value.voluntary_context_switches,
            involuntary_context_switches: value.involuntary_context_switches,
        }
    }
}

fn optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "NONE".to_owned(), |value| value.to_string())
}

unsafe extern "C" {
    fn wait4(pid: c_int, status: *mut c_int, options: c_int, usage: *mut RawResourceUsage)
        -> c_int;
}
