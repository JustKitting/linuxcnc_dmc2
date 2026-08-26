use std::ffi::{c_int, CString};

use dmc2_linuxcnc_interface::{CodeDomain, CMS_STATUS, EMC_NML_MESSAGE_TYPE, NML_ERROR};

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
    pub(super) no_error: i32,
    pub(super) invalid_configuration: i32,
    pub(super) invalid_message: i32,
    pub(super) status_message_type: i32,
    pub(super) cms_status_not_set: i32,
    pub(super) cms_read_old: i32,
    pub(super) cms_read_ok: i32,
}

impl PollCodes {
    pub(super) fn required() -> Self {
        Self {
            no_error: required_nml_error("NML_NO_ERROR"),
            invalid_configuration: required_nml_error("NML_INVALID_CONFIGURATION"),
            invalid_message: required_nml_error("NML_INVALID_MESSAGE_ERROR"),
            status_message_type: required_code(EMC_NML_MESSAGE_TYPE, "EMC_STAT_TYPE"),
            cms_status_not_set: required_code(CMS_STATUS, "CMS_STATUS_NOT_SET"),
            cms_read_old: required_code(CMS_STATUS, "CMS_READ_OLD"),
            cms_read_ok: required_code(CMS_STATUS, "CMS_READ_OK"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TransportStatus {
    pub(super) nml_error: i32,
    pub(super) cms_status: i32,
}

impl TransportStatus {
    pub(super) fn healthy_after_open(self, codes: PollCodes) -> bool {
        // CMS::open() explicitly initializes status to CMS_STATUS_NOT_SET.
        // A read, write, clear, closed, unknown, or error state here is not a
        // successful untouched NML channel, even when its numeric value is
        // nonnegative.
        self.nml_error == codes.no_error && self.cms_status == codes.cms_status_not_set
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PollOutcome {
    pub(super) disposition: PollDisposition,
    pub(super) transport: TransportStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PollDecision {
    disposition: PollDisposition,
    transport: TransportStatus,
    received_status: bool,
    copy_snapshot: bool,
}

fn fault(transport: TransportStatus, received_status: bool) -> PollDecision {
    PollDecision {
        disposition: PollDisposition::Fault,
        transport,
        received_status,
        copy_snapshot: false,
    }
}

fn classify_observation(
    native_result: c_int,
    message_type: i32,
    nml_error: i32,
    cms_status: i32,
    received_status: bool,
    codes: PollCodes,
) -> PollDecision {
    let transport = TransportStatus {
        nml_error,
        cms_status,
    };
    if native_result != DMC2_TASK_STATUS_NATIVE_OK {
        return fault(
            TransportStatus {
                nml_error: codes.invalid_configuration,
                cms_status,
            },
            received_status,
        );
    }
    if nml_error != codes.no_error {
        return fault(transport, received_status);
    }
    if message_type == codes.status_message_type {
        // LinuxCNC 2.9.10 NML::peek() returns a message type only from the
        // CMS_READ_OK branch. Every other pairing is internally inconsistent.
        if cms_status != codes.cms_read_ok {
            return fault(transport, received_status);
        }
        return PollDecision {
            disposition: PollDisposition::Snapshot,
            transport,
            received_status: true,
            copy_snapshot: true,
        };
    }
    if message_type == 0 {
        // LinuxCNC 2.9.10 NML::peek() returns zero only from CMS_READ_OLD.
        if cms_status != codes.cms_read_old {
            return fault(transport, received_status);
        }
        if !received_status {
            return PollDecision {
                disposition: PollDisposition::WaitingForFirstStatus,
                transport,
                received_status: false,
                copy_snapshot: false,
            };
        }
        return PollDecision {
            disposition: PollDisposition::Snapshot,
            transport,
            received_status: true,
            copy_snapshot: true,
        };
    }
    fault(
        TransportStatus {
            nml_error: codes.invalid_message,
            cms_status,
        },
        received_status,
    )
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

pub(super) fn required_cms_status(name: &str) -> i32 {
    required_code(CMS_STATUS, name)
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
    pub(super) fn open(nml_file: &CString, codes: PollCodes) -> (Option<Self>, TransportStatus) {
        let mut nml_error = codes.invalid_configuration;
        let mut cms_status = codes.cms_status_not_set;
        let native =
            unsafe { dmc2_task_status_open(nml_file.as_ptr(), &mut nml_error, &mut cms_status) };
        let transport = TransportStatus {
            nml_error,
            cms_status,
        };
        if native.is_null() {
            (None, transport)
        } else {
            (
                Some(Self {
                    native,
                    received_status: false,
                }),
                transport,
            )
        }
    }

    pub(super) fn poll(&mut self, snapshot: &mut NativeSnapshot, codes: PollCodes) -> PollOutcome {
        let mut message_type = 0;
        let mut nml_error = codes.invalid_configuration;
        let mut cms_status = codes.cms_status_not_set;
        let native_result = unsafe {
            dmc2_task_status_observe(
                self.native,
                &mut message_type,
                &mut nml_error,
                &mut cms_status,
            )
        };
        let decision = classify_observation(
            native_result,
            message_type,
            nml_error,
            cms_status,
            self.received_status,
            codes,
        );
        self.received_status = decision.received_status;
        if decision.copy_snapshot {
            let copy_result = unsafe { dmc2_task_status_copy(self.native, snapshot) };
            if copy_result != DMC2_TASK_STATUS_NATIVE_OK {
                return PollOutcome {
                    disposition: PollDisposition::Fault,
                    transport: TransportStatus {
                        nml_error: codes.invalid_configuration,
                        cms_status,
                    },
                };
            }
            derive_axis_stopped(snapshot);
        }
        PollOutcome {
            disposition: decision.disposition,
            transport: decision.transport,
        }
    }
}

impl Drop for StatusChannel {
    fn drop(&mut self) {
        unsafe { dmc2_task_status_close(self.native) };
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::ptr;

    use super::*;

    #[test]
    fn every_poll_observation_state_transition_is_exact() {
        let codes = PollCodes::required();
        let cms_read_ok = codes.cms_read_ok;
        let cms_read_old = codes.cms_read_old;
        let healthy_transport = TransportStatus {
            nml_error: codes.no_error,
            cms_status: cms_read_ok,
        };
        let snapshot = PollDecision {
            disposition: PollDisposition::Snapshot,
            transport: healthy_transport,
            received_status: true,
            copy_snapshot: true,
        };
        assert_eq!(
            classify_observation(
                DMC2_TASK_STATUS_NATIVE_OK,
                codes.status_message_type,
                codes.no_error,
                cms_read_ok,
                false,
                codes,
            ),
            snapshot
        );
        assert_eq!(
            classify_observation(
                DMC2_TASK_STATUS_NATIVE_OK,
                0,
                codes.no_error,
                cms_read_old,
                false,
                codes,
            ),
            PollDecision {
                disposition: PollDisposition::WaitingForFirstStatus,
                transport: TransportStatus {
                    nml_error: codes.no_error,
                    cms_status: cms_read_old,
                },
                received_status: false,
                copy_snapshot: false,
            }
        );
        assert_eq!(
            classify_observation(
                DMC2_TASK_STATUS_NATIVE_OK,
                0,
                codes.no_error,
                cms_read_old,
                true,
                codes,
            ),
            PollDecision {
                disposition: PollDisposition::Snapshot,
                transport: TransportStatus {
                    nml_error: codes.no_error,
                    cms_status: cms_read_old,
                },
                received_status: true,
                copy_snapshot: true,
            }
        );
        for received_status in [false, true] {
            for message_type in [i32::MIN, -1, 1, i32::MAX] {
                assert_eq!(
                    classify_observation(
                        DMC2_TASK_STATUS_NATIVE_OK,
                        message_type,
                        codes.no_error,
                        cms_read_ok,
                        received_status,
                        codes,
                    ),
                    fault(
                        TransportStatus {
                            nml_error: codes.invalid_message,
                            cms_status: cms_read_ok,
                        },
                        received_status,
                    )
                );
            }
            for message_type in [i32::MIN, 0, codes.status_message_type, i32::MAX] {
                assert_eq!(
                    classify_observation(
                        DMC2_TASK_STATUS_NATIVE_OK,
                        message_type,
                        3,
                        cms_read_ok,
                        received_status,
                        codes,
                    ),
                    fault(
                        TransportStatus {
                            nml_error: 3,
                            cms_status: cms_read_ok,
                        },
                        received_status,
                    )
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
                        cms_read_ok,
                        received_status,
                        codes,
                    ),
                    fault(
                        TransportStatus {
                            nml_error: codes.invalid_configuration,
                            cms_status: cms_read_ok,
                        },
                        received_status,
                    )
                );
            }
        }
    }

    #[test]
    fn every_cms_status_and_unknown_value_is_classified_exactly() {
        let codes = PollCodes::required();
        for entry in CMS_STATUS.codes {
            let cms_status: i32 = entry.code.try_into().expect("CMS status fits i32");
            let transport = TransportStatus {
                nml_error: codes.no_error,
                cms_status,
            };
            let decision = classify_observation(
                DMC2_TASK_STATUS_NATIVE_OK,
                codes.status_message_type,
                codes.no_error,
                cms_status,
                false,
                codes,
            );
            if cms_status == codes.cms_read_ok {
                assert_eq!(
                    decision,
                    PollDecision {
                        disposition: PollDisposition::Snapshot,
                        transport,
                        received_status: true,
                        copy_snapshot: true,
                    },
                    "{}",
                    entry.name
                );
            } else {
                assert_eq!(decision, fault(transport, false), "{}", entry.name);
            }

            let no_message = classify_observation(
                DMC2_TASK_STATUS_NATIVE_OK,
                0,
                codes.no_error,
                cms_status,
                false,
                codes,
            );
            if cms_status == codes.cms_read_old {
                assert_eq!(
                    no_message,
                    PollDecision {
                        disposition: PollDisposition::WaitingForFirstStatus,
                        transport,
                        received_status: false,
                        copy_snapshot: false,
                    },
                    "{}",
                    entry.name
                );
            } else {
                assert_eq!(no_message, fault(transport, false), "{}", entry.name);
            }

            assert_eq!(
                transport.healthy_after_open(codes),
                cms_status == codes.cms_status_not_set,
                "{}",
                entry.name
            );
        }

        for cms_status in [i32::MIN, 7, i32::MAX] {
            let transport = TransportStatus {
                nml_error: codes.no_error,
                cms_status,
            };
            assert_eq!(
                classify_observation(
                    DMC2_TASK_STATUS_NATIVE_OK,
                    codes.status_message_type,
                    codes.no_error,
                    cms_status,
                    false,
                    codes,
                ),
                fault(transport, false)
            );
            assert!(!transport.healthy_after_open(codes));
        }
    }

    #[test]
    fn every_nml_error_and_unknown_value_is_classified_exactly() {
        let codes = PollCodes::required();
        let nml_errors = NML_ERROR
            .codes
            .iter()
            .map(|entry| i32::try_from(entry.code).unwrap())
            .chain([i32::MIN, i32::MAX])
            .collect::<BTreeSet<_>>();
        for nml_error in nml_errors {
            for (message_type, cms_status) in [
                (codes.status_message_type, codes.cms_read_ok),
                (0, codes.cms_read_old),
                (77, codes.cms_read_ok),
            ] {
                let transport = TransportStatus {
                    nml_error,
                    cms_status,
                };
                let decision = classify_observation(
                    DMC2_TASK_STATUS_NATIVE_OK,
                    message_type,
                    nml_error,
                    cms_status,
                    false,
                    codes,
                );
                if nml_error != codes.no_error {
                    assert_eq!(decision, fault(transport, false));
                }
            }
            assert_eq!(
                TransportStatus {
                    nml_error,
                    cms_status: codes.cms_status_not_set,
                }
                .healthy_after_open(codes),
                nml_error == codes.no_error
            );
        }
    }

    #[test]
    fn native_channel_argument_guards_fail_closed_without_opening_nml() {
        let codes = PollCodes::required();
        let mut nml_error = codes.no_error;
        let mut cms_status = required_cms_status("CMS_READ_OK");

        let null_channel =
            unsafe { dmc2_task_status_open(ptr::null(), &mut nml_error, &mut cms_status) };
        assert!(null_channel.is_null());
        assert_eq!(nml_error, codes.invalid_configuration);
        assert_eq!(cms_status, codes.cms_status_not_set);

        let empty = CString::new("").unwrap();
        nml_error = codes.no_error;
        cms_status = required_cms_status("CMS_READ_OK");
        let empty_channel =
            unsafe { dmc2_task_status_open(empty.as_ptr(), &mut nml_error, &mut cms_status) };
        assert!(empty_channel.is_null());
        assert_eq!(nml_error, codes.invalid_configuration);
        assert_eq!(cms_status, codes.cms_status_not_set);

        let mut message_type = 42;
        nml_error = codes.no_error;
        cms_status = required_cms_status("CMS_READ_OK");
        assert_eq!(
            unsafe {
                dmc2_task_status_observe(
                    ptr::null_mut(),
                    &mut message_type,
                    &mut nml_error,
                    &mut cms_status,
                )
            },
            DMC2_TASK_STATUS_NATIVE_ERROR
        );
        assert_eq!(message_type, 0);
        assert_eq!(nml_error, codes.invalid_configuration);
        assert_eq!(cms_status, codes.cms_status_not_set);
        assert_eq!(
            unsafe {
                dmc2_task_status_observe(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            },
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
        assert!(
            unsafe { dmc2_task_status_open(ptr::null(), ptr::null_mut(), ptr::null_mut()) }
                .is_null()
        );
        unsafe { dmc2_task_status_close(ptr::null_mut()) };
    }
}
