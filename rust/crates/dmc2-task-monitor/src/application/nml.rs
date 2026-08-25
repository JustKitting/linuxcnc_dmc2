use std::ffi::{c_int, CString};

use dmc2_linuxcnc_interface::{CodeDomain, EMC_NML_MESSAGE_TYPE, NML_ERROR};

use crate::snapshot::{
    derive_axis_stopped, dmc2_task_status_close, dmc2_task_status_copy, dmc2_task_status_observe,
    dmc2_task_status_open, dmc2_task_status_snapshot_abi_version, dmc2_task_status_snapshot_size,
    NativeSnapshot, NativeTaskStatusChannel, DMC2_TASK_STATUS_NATIVE_OK,
};

#[cfg(test)]
use crate::snapshot::DMC2_TASK_STATUS_NATIVE_ERROR;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PollDisposition {
    Snapshot,
    WaitingForFirstStatus,
    Fault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PollCodes {
    no_error: i32,
    invalid_configuration: i32,
    invalid_message: i32,
    status_message_type: i32,
}

impl PollCodes {
    pub(super) fn required() -> Self {
        Self {
            no_error: required_nml_error("NML_NO_ERROR"),
            invalid_configuration: required_nml_error("NML_INVALID_CONFIGURATION"),
            invalid_message: required_nml_error("NML_INVALID_MESSAGE_ERROR"),
            status_message_type: required_code(EMC_NML_MESSAGE_TYPE, "EMC_STAT_TYPE"),
        }
    }

    pub(super) const fn no_error(self) -> i32 {
        self.no_error
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PollDecision {
    disposition: PollDisposition,
    nml_error: i32,
    received_status: bool,
    copy_snapshot: bool,
}

fn fault(nml_error: i32, received_status: bool) -> PollDecision {
    PollDecision {
        disposition: PollDisposition::Fault,
        nml_error,
        received_status,
        copy_snapshot: false,
    }
}

fn classify_observation(
    native_result: c_int,
    message_type: i32,
    nml_error: i32,
    received_status: bool,
    codes: PollCodes,
) -> PollDecision {
    if native_result != DMC2_TASK_STATUS_NATIVE_OK {
        return fault(codes.invalid_configuration, received_status);
    }
    if nml_error != codes.no_error {
        return fault(nml_error, received_status);
    }
    if message_type == codes.status_message_type {
        return PollDecision {
            disposition: PollDisposition::Snapshot,
            nml_error: codes.no_error,
            received_status: true,
            copy_snapshot: true,
        };
    }
    if message_type == 0 && !received_status {
        return PollDecision {
            disposition: PollDisposition::WaitingForFirstStatus,
            nml_error: codes.no_error,
            received_status: false,
            copy_snapshot: false,
        };
    }
    if message_type != 0 {
        return fault(codes.invalid_message, received_status);
    }
    PollDecision {
        disposition: PollDisposition::Snapshot,
        nml_error: codes.no_error,
        received_status: true,
        copy_snapshot: true,
    }
}

fn required_code(domain: CodeDomain, name: &str) -> i32 {
    domain
        .codes
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("LinuxCNC 2.9.10 controller interface omitted {name}"))
        .code
        .try_into()
        .unwrap_or_else(|_| panic!("LinuxCNC 2.9.10 value {name} does not fit in i32"))
}

pub(super) fn required_nml_error(name: &str) -> i32 {
    required_code(NML_ERROR, name)
}

pub(super) fn snapshot_abi_version() -> u32 {
    unsafe { dmc2_task_status_snapshot_abi_version() }
}

pub(super) fn snapshot_size() -> usize {
    unsafe { dmc2_task_status_snapshot_size() }
}

pub(super) struct StatusChannel {
    native: *mut NativeTaskStatusChannel,
    received_status: bool,
}

impl StatusChannel {
    pub(super) fn open(nml_file: &CString, codes: PollCodes) -> (Option<Self>, i32) {
        let mut nml_error = codes.invalid_configuration;
        let native = unsafe { dmc2_task_status_open(nml_file.as_ptr(), &mut nml_error) };
        if native.is_null() {
            (None, nml_error)
        } else {
            (
                Some(Self {
                    native,
                    received_status: false,
                }),
                nml_error,
            )
        }
    }

    pub(super) fn poll(
        &mut self,
        snapshot: &mut NativeSnapshot,
        codes: PollCodes,
    ) -> (PollDisposition, i32) {
        let mut message_type = 0;
        let mut nml_error = codes.invalid_configuration;
        let native_result =
            unsafe { dmc2_task_status_observe(self.native, &mut message_type, &mut nml_error) };
        let decision = classify_observation(
            native_result,
            message_type,
            nml_error,
            self.received_status,
            codes,
        );
        self.received_status = decision.received_status;
        if decision.copy_snapshot {
            let copy_result = unsafe { dmc2_task_status_copy(self.native, snapshot) };
            if copy_result != DMC2_TASK_STATUS_NATIVE_OK {
                return (PollDisposition::Fault, codes.invalid_configuration);
            }
            derive_axis_stopped(snapshot);
        }
        (decision.disposition, decision.nml_error)
    }
}

impl Drop for StatusChannel {
    fn drop(&mut self) {
        unsafe { dmc2_task_status_close(self.native) };
    }
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;

    #[test]
    fn every_poll_observation_state_transition_is_exact() {
        let codes = PollCodes::required();
        let snapshot = PollDecision {
            disposition: PollDisposition::Snapshot,
            nml_error: codes.no_error,
            received_status: true,
            copy_snapshot: true,
        };
        assert_eq!(
            classify_observation(
                DMC2_TASK_STATUS_NATIVE_OK,
                codes.status_message_type,
                codes.no_error,
                false,
                codes,
            ),
            snapshot
        );
        assert_eq!(
            classify_observation(DMC2_TASK_STATUS_NATIVE_OK, 0, codes.no_error, false, codes,),
            PollDecision {
                disposition: PollDisposition::WaitingForFirstStatus,
                nml_error: codes.no_error,
                received_status: false,
                copy_snapshot: false,
            }
        );
        assert_eq!(
            classify_observation(DMC2_TASK_STATUS_NATIVE_OK, 0, codes.no_error, true, codes,),
            snapshot
        );
        for received_status in [false, true] {
            for message_type in [i32::MIN, -1, 1, i32::MAX] {
                assert_eq!(
                    classify_observation(
                        DMC2_TASK_STATUS_NATIVE_OK,
                        message_type,
                        codes.no_error,
                        received_status,
                        codes,
                    ),
                    fault(codes.invalid_message, received_status)
                );
            }
            for message_type in [i32::MIN, 0, codes.status_message_type, i32::MAX] {
                assert_eq!(
                    classify_observation(
                        DMC2_TASK_STATUS_NATIVE_OK,
                        message_type,
                        3,
                        received_status,
                        codes,
                    ),
                    fault(3, received_status)
                );
            }
            for native_result in [
                c_int::MIN,
                DMC2_TASK_STATUS_NATIVE_ERROR,
                DMC2_TASK_STATUS_NATIVE_OK + 1,
                c_int::MAX,
            ] {
                assert_eq!(
                    classify_observation(
                        native_result,
                        codes.status_message_type,
                        codes.no_error,
                        received_status,
                        codes,
                    ),
                    fault(codes.invalid_configuration, received_status)
                );
            }
        }
    }

    #[test]
    fn native_channel_argument_guards_fail_closed_without_opening_nml() {
        let codes = PollCodes::required();
        let mut nml_error = codes.no_error;

        let null_channel = unsafe { dmc2_task_status_open(ptr::null(), &mut nml_error) };
        assert!(null_channel.is_null());
        assert_eq!(nml_error, codes.invalid_configuration);

        let empty = CString::new("").unwrap();
        nml_error = codes.no_error;
        let empty_channel = unsafe { dmc2_task_status_open(empty.as_ptr(), &mut nml_error) };
        assert!(empty_channel.is_null());
        assert_eq!(nml_error, codes.invalid_configuration);

        let mut message_type = 42;
        nml_error = codes.no_error;
        assert_eq!(
            unsafe { dmc2_task_status_observe(ptr::null_mut(), &mut message_type, &mut nml_error) },
            DMC2_TASK_STATUS_NATIVE_ERROR
        );
        assert_eq!(message_type, 0);
        assert_eq!(nml_error, codes.invalid_configuration);
        assert_eq!(
            unsafe { dmc2_task_status_observe(ptr::null_mut(), ptr::null_mut(), ptr::null_mut()) },
            DMC2_TASK_STATUS_NATIVE_ERROR
        );

        let mut snapshot = NativeSnapshot::safe();
        assert_eq!(
            unsafe { dmc2_task_status_copy(ptr::null_mut(), &mut snapshot) },
            DMC2_TASK_STATUS_NATIVE_ERROR
        );
        assert_eq!(
            unsafe { dmc2_task_status_copy(ptr::null_mut(), ptr::null_mut()) },
            DMC2_TASK_STATUS_NATIVE_ERROR
        );
        assert!(unsafe { dmc2_task_status_open(ptr::null(), ptr::null_mut()) }.is_null());
        unsafe { dmc2_task_status_close(ptr::null_mut()) };
    }
}
