use std::io;
use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_long};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, ExitStatus};

use crate::event::{signal_name, Event};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitEvidence {
    pub pid: u32,
    pub status: ExitStatus,
    pub usage: ResourceUsage,
}

impl WaitEvidence {
    pub fn event_fields(self, event: Event) -> Event {
        let status = self.status;
        let usage = self.usage;
        event
            .field("child_pid", self.pid)
            .field("raw_wait_status", status.into_raw())
            .field("exit_code", optional_i32(status.code()))
            .field("signal", optional_i32(status.signal()))
            .field(
                "signal_name",
                status.signal().map(signal_name).unwrap_or("NONE"),
            )
            .field("core_dumped", status.core_dumped())
            .field("rusage_user_seconds", usage.user_seconds)
            .field("rusage_user_microseconds", usage.user_microseconds)
            .field("rusage_system_seconds", usage.system_seconds)
            .field("rusage_system_microseconds", usage.system_microseconds)
            .field("rusage_max_resident_kib", usage.max_resident_kib)
            .field(
                "rusage_integral_shared_memory_raw",
                usage.integral_shared_memory_raw,
            )
            .field(
                "rusage_integral_unshared_data_raw",
                usage.integral_unshared_data_raw,
            )
            .field(
                "rusage_integral_unshared_stack_raw",
                usage.integral_unshared_stack_raw,
            )
            .field("rusage_minor_faults", usage.minor_faults)
            .field("rusage_major_faults", usage.major_faults)
            .field("rusage_swaps", usage.swaps)
            .field("rusage_block_reads", usage.block_reads)
            .field("rusage_block_writes", usage.block_writes)
            .field("rusage_ipc_messages_sent", usage.ipc_messages_sent)
            .field("rusage_ipc_messages_received", usage.ipc_messages_received)
            .field("rusage_signals_delivered", usage.signals_delivered)
            .field(
                "rusage_voluntary_context_switches",
                usage.voluntary_context_switches,
            )
            .field(
                "rusage_involuntary_context_switches",
                usage.involuntary_context_switches,
            )
    }

    pub fn kernel_outcome(self) -> &'static str {
        match (self.status.signal(), self.status.code()) {
            (Some(_), _) => "kernel-signal-termination",
            (None, Some(0)) => "zero-exit",
            (None, Some(_)) => "nonzero-exit",
            _ => "unknown-wait-status",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnyWait {
    Running,
    Exited(WaitEvidence),
    NoChildren,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceUsage {
    pub user_seconds: c_long,
    pub user_microseconds: c_long,
    pub system_seconds: c_long,
    pub system_microseconds: c_long,
    pub max_resident_kib: c_long,
    pub integral_shared_memory_raw: c_long,
    pub integral_unshared_data_raw: c_long,
    pub integral_unshared_stack_raw: c_long,
    pub minor_faults: c_long,
    pub major_faults: c_long,
    pub swaps: c_long,
    pub block_reads: c_long,
    pub block_writes: c_long,
    pub ipc_messages_sent: c_long,
    pub ipc_messages_received: c_long,
    pub signals_delivered: c_long,
    pub voluntary_context_switches: c_long,
    pub involuntary_context_switches: c_long,
}

pub fn wait(child: &mut Child) -> io::Result<WaitEvidence> {
    let pid = i32::try_from(child.id()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("child PID {} does not fit pid_t", child.id()),
        )
    })?;
    loop {
        let mut status = 0_i32;
        let mut raw_usage = MaybeUninit::<RawResourceUsage>::zeroed();
        // SAFETY: `status` and `raw_usage` are valid writable objects, `pid`
        // names the direct child returned by `Command::spawn`, and options=0
        // requests one blocking terminal-state result without retaining either
        // pointer after the call.
        let result = unsafe { wait4(pid, &mut status, 0, raw_usage.as_mut_ptr()) };
        if result == pid {
            // SAFETY: wait4 returned success and initialized the rusage object.
            let raw_usage = unsafe { raw_usage.assume_init() };
            return Ok(WaitEvidence {
                pid: u32::try_from(result).expect("positive pid_t fits u32"),
                status: ExitStatus::from_raw(status),
                usage: raw_usage.into(),
            });
        }
        if result >= 0 {
            return Err(io::Error::other(format!(
                "wait4 returned PID {result} while waiting for direct child {pid}"
            )));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

pub fn wait_any_nonblocking() -> io::Result<AnyWait> {
    const WNOHANG: c_int = 1;
    const ECHILD_LINUX: i32 = 10;
    loop {
        let mut status = 0_i32;
        let mut raw_usage = MaybeUninit::<RawResourceUsage>::zeroed();
        // SAFETY: both pointers refer to writable objects for the duration of
        // the call. pid=-1 selects any child, and WNOHANG prevents blocking.
        let result = unsafe { wait4(-1, &mut status, WNOHANG, raw_usage.as_mut_ptr()) };
        if result > 0 {
            // SAFETY: wait4 returned a child PID and initialized rusage.
            let raw_usage = unsafe { raw_usage.assume_init() };
            return Ok(AnyWait::Exited(WaitEvidence {
                pid: u32::try_from(result).expect("positive pid_t fits u32"),
                status: ExitStatus::from_raw(status),
                usage: raw_usage.into(),
            }));
        }
        if result == 0 {
            return Ok(AnyWait::Running);
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        // This binary is Linux-only: ECHILD is 10 in the Linux errno ABI used
        // by the pinned Raspberry Pi/aarch64 deployment.
        if error.raw_os_error() == Some(ECHILD_LINUX) {
            return Ok(AnyWait::NoChildren);
        }
        return Err(error);
    }
}

fn optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "NONE".to_owned(), |value| value.to_string())
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
            integral_shared_memory_raw: value.shared_text_raw,
            integral_unshared_data_raw: value.unshared_data_raw,
            integral_unshared_stack_raw: value.unshared_stack_raw,
            minor_faults: value.minor_faults,
            major_faults: value.major_faults,
            swaps: value.swaps,
            block_reads: value.block_reads,
            block_writes: value.block_writes,
            ipc_messages_sent: value.ipc_messages_sent,
            ipc_messages_received: value.ipc_messages_received,
            signals_delivered: value.signals_delivered,
            voluntary_context_switches: value.voluntary_context_switches,
            involuntary_context_switches: value.involuntary_context_switches,
        }
    }
}

unsafe extern "C" {
    fn wait4(pid: c_int, status: *mut c_int, options: c_int, usage: *mut RawResourceUsage)
        -> c_int;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn returns_real_kernel_status_and_resource_usage() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 37"])
            .spawn()
            .expect("spawn child");
        let evidence = wait(&mut child).expect("wait for child");

        assert_eq!(evidence.status.code(), Some(37));
        assert!(evidence.usage.user_seconds >= 0);
        assert!(evidence.usage.system_seconds >= 0);
    }
}
