use std::ffi::{c_int, CString};

use dmc2_linuxcnc_interface::NML_ERROR;

#[cfg(test)]
use crate::snapshot::DMC2_TASK_STATUS_POLL_ERROR;
use crate::snapshot::{
    dmc2_task_status_close, dmc2_task_status_open, dmc2_task_status_poll,
    dmc2_task_status_snapshot_abi_version, dmc2_task_status_snapshot_size, NativeSnapshot,
    NativeTaskStatusChannel, DMC2_TASK_STATUS_POLL_NOT_READY, DMC2_TASK_STATUS_POLL_OK,
};

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
        DMC2_TASK_STATUS_POLL_OK => PollDisposition::Snapshot,
        DMC2_TASK_STATUS_POLL_NOT_READY => PollDisposition::WaitingForFirstStatus,
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
    use std::ptr;

    use super::*;

    #[test]
    fn every_native_poll_result_and_nml_error_precedence_is_exact() {
        let no_error = 0;
        assert_eq!(DMC2_TASK_STATUS_POLL_ERROR, -1);
        assert_eq!(DMC2_TASK_STATUS_POLL_OK, 0);
        assert_eq!(DMC2_TASK_STATUS_POLL_NOT_READY, 1);
        assert_eq!(
            poll_disposition(DMC2_TASK_STATUS_POLL_NOT_READY, no_error, no_error),
            PollDisposition::WaitingForFirstStatus
        );
        assert_eq!(
            poll_disposition(DMC2_TASK_STATUS_POLL_OK, no_error, no_error),
            PollDisposition::Snapshot
        );
        for result in [
            c_int::MIN,
            DMC2_TASK_STATUS_POLL_ERROR,
            DMC2_TASK_STATUS_POLL_NOT_READY + 1,
            c_int::MAX,
        ] {
            assert_eq!(
                poll_disposition(result, no_error, no_error),
                PollDisposition::Fault
            );
        }
        for result in [
            c_int::MIN,
            DMC2_TASK_STATUS_POLL_ERROR,
            DMC2_TASK_STATUS_POLL_OK,
            DMC2_TASK_STATUS_POLL_NOT_READY,
            c_int::MAX,
        ] {
            assert_eq!(
                poll_disposition(result, 3, no_error),
                PollDisposition::Fault
            );
        }
    }

    #[test]
    fn native_channel_argument_guards_fail_closed_without_opening_nml() {
        let invalid_configuration = required_nml_error("NML_INVALID_CONFIGURATION");
        let no_error = required_nml_error("NML_NO_ERROR");
        let mut nml_error = no_error;

        let null_channel = unsafe { dmc2_task_status_open(ptr::null(), &mut nml_error) };
        assert!(null_channel.is_null());
        assert_eq!(nml_error, invalid_configuration);

        let empty = CString::new("").unwrap();
        nml_error = no_error;
        let empty_channel = unsafe { dmc2_task_status_open(empty.as_ptr(), &mut nml_error) };
        assert!(empty_channel.is_null());
        assert_eq!(nml_error, invalid_configuration);

        let mut snapshot = NativeSnapshot::safe();
        nml_error = no_error;
        let result =
            unsafe { dmc2_task_status_poll(ptr::null_mut(), &mut snapshot, &mut nml_error) };
        assert_eq!(result, DMC2_TASK_STATUS_POLL_ERROR);
        assert_eq!(nml_error, invalid_configuration);

        assert!(unsafe { dmc2_task_status_open(ptr::null(), ptr::null_mut()) }.is_null());
        assert_eq!(
            unsafe { dmc2_task_status_poll(ptr::null_mut(), ptr::null_mut(), ptr::null_mut()) },
            DMC2_TASK_STATUS_POLL_ERROR
        );
        unsafe { dmc2_task_status_close(ptr::null_mut()) };
    }
}
