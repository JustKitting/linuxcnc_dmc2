use std::io;
use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_long, c_uint};
use std::os::unix::process::ExitStatusExt;
use std::process::Child;
use std::process::ExitStatus;

use crate::event::{signal_name, Event};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitEvidence {
    pub pid: u32,
    pub status: ExitStatus,
    pub usage: ResourceUsage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalObservation {
    pub pid: u32,
    pub signal: c_int,
    pub error: c_int,
    pub code: c_int,
    pub uid: c_uint,
    pub status: c_int,
    pub user_ticks: c_long,
    pub system_ticks: c_long,
}

impl TerminalObservation {
    pub fn code_name(self) -> &'static str {
        waitid_code_name(self.code)
    }

    pub fn core_dumped(self) -> bool {
        self.code == CLD_DUMPED
    }

    pub fn event_fields(self, event: Event) -> Event {
        event
            .field("waitid_pid", self.pid)
            .field("waitid_signal", self.signal)
            .field("waitid_error", self.error)
            .field("waitid_code", self.code)
            .field("waitid_code_name", waitid_code_name(self.code))
            .field("waitid_uid", self.uid)
            .field("waitid_status", self.status)
            .field("waitid_user_ticks", self.user_ticks)
            .field("waitid_system_ticks", self.system_ticks)
    }

    pub fn agrees_with(self, evidence: WaitEvidence) -> bool {
        if self.pid != evidence.pid {
            return false;
        }
        match self.code {
            CLD_EXITED => evidence.status.code() == Some(self.status),
            CLD_KILLED => {
                evidence.status.signal() == Some(self.status) && !evidence.status.core_dumped()
            }
            CLD_DUMPED => {
                evidence.status.signal() == Some(self.status) && evidence.status.core_dumped()
            }
            _ => false,
        }
    }
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnyWait {
    Running,
    Terminal(TerminalObservation),
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

#[cfg(test)]
pub fn observe(child: &Child) -> io::Result<TerminalObservation> {
    let pid = i32::try_from(child.id()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("child PID {} does not fit pid_t", child.id()),
        )
    })?;
    observe_id(
        P_PID,
        u32::try_from(pid).expect("positive pid_t fits u32"),
        false,
    )?
    .ok_or_else(|| {
        io::Error::other(format!(
            "blocking waitid returned no event for direct child {pid}"
        ))
    })
}

pub fn observe_any_nonblocking() -> io::Result<AnyWait> {
    const ECHILD_LINUX: i32 = 10;
    match observe_id(P_ALL, 0, true) {
        Ok(Some(observation)) => Ok(AnyWait::Terminal(observation)),
        Ok(None) => Ok(AnyWait::Running),
        Err(error) if error.raw_os_error() == Some(ECHILD_LINUX) => Ok(AnyWait::NoChildren),
        Err(error) => Err(error),
    }
}

pub fn observe_pid_nonblocking(pid: u32) -> io::Result<Option<TerminalObservation>> {
    let pid = i32::try_from(pid).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("child PID {pid} does not fit pid_t"),
        )
    })?;
    observe_id(
        P_PID,
        u32::try_from(pid).expect("positive pid_t fits u32"),
        true,
    )
}

pub fn reap(child: &mut Child) -> io::Result<WaitEvidence> {
    reap_pid(child.id())
}

pub fn reap_pid(pid: u32) -> io::Result<WaitEvidence> {
    let pid = i32::try_from(pid).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("child PID {pid} does not fit pid_t"),
        )
    })?;
    loop {
        let mut status = 0_i32;
        let mut raw_usage = MaybeUninit::<RawResourceUsage>::zeroed();
        // SAFETY: `status` and `raw_usage` are valid writable objects, `pid`
        // names a waitable child previously reported by waitid, and options=0
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
                "wait4 returned PID {result} while reaping child {pid}"
            )));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn observe_id(
    id_type: c_int,
    id: c_uint,
    nonblocking: bool,
) -> io::Result<Option<TerminalObservation>> {
    loop {
        let mut information = MaybeUninit::<RawSignalInformation>::zeroed();
        let options = WEXITED | WNOWAIT | if nonblocking { WNOHANG } else { 0 };
        // SAFETY: `information` points to a correctly sized and aligned Linux
        // siginfo_t representation. WNOWAIT deliberately leaves the reported
        // child waitable so its terminal /proc snapshot can be captured before
        // a separate wait4 obtains the authoritative status and rusage.
        let result = unsafe { waitid(id_type, id, information.as_mut_ptr(), options) };
        if result == 0 {
            // SAFETY: a successful waitid initializes siginfo_t. With WNOHANG,
            // Linux reports no available event by leaving si_pid equal to zero.
            let information = unsafe { information.assume_init() };
            if information.pid == 0 {
                return Ok(None);
            }
            return Ok(Some(information.into()));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn waitid_code_name(code: c_int) -> &'static str {
    match code {
        CLD_EXITED => "CLD_EXITED",
        CLD_KILLED => "CLD_KILLED",
        CLD_DUMPED => "CLD_DUMPED",
        CLD_TRAPPED => "CLD_TRAPPED",
        CLD_STOPPED => "CLD_STOPPED",
        CLD_CONTINUED => "CLD_CONTINUED",
        _ => "UNKNOWN_CLD_CODE",
    }
}

const P_ALL: c_int = 0;
const P_PID: c_int = 1;
const WNOHANG: c_int = 1;
const WEXITED: c_int = 4;
const WNOWAIT: c_int = 0x0100_0000;
const CLD_EXITED: c_int = 1;
const CLD_KILLED: c_int = 2;
const CLD_DUMPED: c_int = 3;
const CLD_TRAPPED: c_int = 4;
const CLD_STOPPED: c_int = 5;
const CLD_CONTINUED: c_int = 6;

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

// Linux fixes siginfo_t at 128 bytes. On the 64-bit aarch64 deployment its
// SIGCHLD payload begins at byte 16 and uses 64-bit clock_t values. Keeping the
// complete ABI object here avoids depending on a generated C shim in the
// process owner while still preserving the waitid fields Linux supplies.
#[repr(C)]
#[derive(Clone, Copy)]
struct RawSignalInformation {
    signal: c_int,
    error: c_int,
    code: c_int,
    header_padding: c_int,
    pid: c_int,
    uid: c_uint,
    status: c_int,
    payload_padding: c_int,
    user_ticks: c_long,
    system_ticks: c_long,
    remaining: [u8; 80],
}

impl From<RawSignalInformation> for TerminalObservation {
    fn from(value: RawSignalInformation) -> Self {
        Self {
            pid: u32::try_from(value.pid).expect("waitid returned positive child PID"),
            signal: value.signal,
            error: value.error,
            code: value.code,
            uid: value.uid,
            status: value.status,
            user_ticks: value.user_ticks,
            system_ticks: value.system_ticks,
        }
    }
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
    fn waitid(
        id_type: c_int,
        id: c_uint,
        information: *mut RawSignalInformation,
        options: c_int,
    ) -> c_int;
    fn wait4(pid: c_int, status: *mut c_int, options: c_int, usage: *mut RawResourceUsage)
        -> c_int;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn retains_a_terminal_child_for_proc_capture_before_reaping_it() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 37"])
            .spawn()
            .expect("spawn child");
        let pid = child.id();
        let observation = observe(&child).expect("observe terminal child");
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .expect("terminal child remains visible in proc before reap");

        assert_eq!(observation.pid, pid);
        assert_eq!(observation.code, CLD_EXITED);
        assert_eq!(observation.status, 37);
        assert!(
            stat.contains(") Z "),
            "terminal state was not zombie: {stat}"
        );

        let evidence = reap(&mut child).expect("reap child");

        assert_eq!(evidence.status.code(), Some(37));
        assert!(observation.agrees_with(evidence));
        assert!(evidence.usage.user_seconds >= 0);
        assert!(evidence.usage.system_seconds >= 0);
        assert!(
            std::fs::metadata(format!("/proc/{pid}")).is_err(),
            "reaped child remained in proc"
        );
    }

    #[test]
    fn raw_siginfo_layout_matches_the_linux_abi() {
        assert_eq!(std::mem::size_of::<RawSignalInformation>(), 128);
        assert_eq!(std::mem::align_of::<RawSignalInformation>(), 8);
    }
}
