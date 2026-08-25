use std::ffi::{c_int, CString};

use dmc2_linuxcnc_interface::NML_ERROR;

use crate::snapshot::{
    dmc2_task_status_close, dmc2_task_status_open, dmc2_task_status_poll,
    dmc2_task_status_snapshot_abi_version, dmc2_task_status_snapshot_size, NativeSnapshot,
    NativeTaskStatusChannel,
};

const TASK_STATUS_POLL_OK: c_int = 0;
const TASK_STATUS_POLL_NOT_READY: c_int = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PollDisposition {
    Snapshot,
    WaitingForFirstStatus,
    Fault,
}

fn poll_disposition(result: c_int, nml_error: i32, no_nml_error: i32) -> PollDisposition {
    if nml_error != no_nml_error {
        return PollDisposition::Fault;
    }
    match result {
        TASK_STATUS_POLL_OK => PollDisposition::Snapshot,
        TASK_STATUS_POLL_NOT_READY => PollDisposition::WaitingForFirstStatus,
        _ => PollDisposition::Fault,
    }
}

pub(super) fn snapshot_abi_version() -> u32 {
    unsafe { dmc2_task_status_snapshot_abi_version() }
}

pub(super) fn snapshot_size() -> usize {
    unsafe { dmc2_task_status_snapshot_size() }
}

pub(super) fn required_nml_error(name: &str) -> i32 {
    NML_ERROR
        .codes
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("LinuxCNC 2.9.10 catalog omitted {name}"))
        .code
        .try_into()
        .unwrap_or_else(|_| panic!("LinuxCNC 2.9.10 NML error {name} does not fit in i32"))
}

pub(super) struct StatusChannel(*mut NativeTaskStatusChannel);

impl StatusChannel {
    pub(super) fn open(nml_file: &CString, initial_error: i32) -> (Option<Self>, i32) {
        let mut nml_error = initial_error;
        let channel = unsafe { dmc2_task_status_open(nml_file.as_ptr(), &mut nml_error) };
        if channel.is_null() {
            (None, nml_error)
        } else {
            (Some(Self(channel)), nml_error)
        }
    }

    pub(super) fn poll(
        &mut self,
        snapshot: &mut NativeSnapshot,
        initial_error: i32,
        no_nml_error: i32,
    ) -> (PollDisposition, i32) {
        let mut nml_error = initial_error;
        let result = unsafe { dmc2_task_status_poll(self.0, snapshot, &mut nml_error) };
        (poll_disposition(result, nml_error, no_nml_error), nml_error)
    }
}

impl Drop for StatusChannel {
    fn drop(&mut self) {
        unsafe { dmc2_task_status_close(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_status_startup_latency_is_not_a_fault_or_snapshot() {
        let no_error = 0;
        assert_eq!(
            poll_disposition(TASK_STATUS_POLL_NOT_READY, no_error, no_error),
            PollDisposition::WaitingForFirstStatus
        );
        assert_eq!(
            poll_disposition(TASK_STATUS_POLL_OK, no_error, no_error),
            PollDisposition::Snapshot
        );
        assert_eq!(
            poll_disposition(TASK_STATUS_POLL_NOT_READY, 3, no_error),
            PollDisposition::Fault
        );
        assert_eq!(
            poll_disposition(-1, no_error, no_error),
            PollDisposition::Fault
        );
    }
}
