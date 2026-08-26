use std::ffi::CString;
use std::fmt;

use dmc2_linuxcnc_interface::ERROR_MESSAGE_CONTRACTS;

use crate::application::nml::{PollCodes, TransportStatus};

use super::record::{DecodeError, ErrorMessageRecord};

#[allow(
    dead_code,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals
)]
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/error_message_bindings.rs"));
}

pub(in crate::application) type RawErrorSnapshot = bindings::dmc2_error_message_snapshot;
pub(in crate::application) const ERROR_MESSAGE_ABI_VERSION: u32 =
    bindings::DMC2_ERROR_MESSAGE_ABI_VERSION;
pub(super) const ERROR_OBJECT_CAPACITY: usize =
    bindings::DMC2_ERROR_MESSAGE_OBJECT_CAPACITY as usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeResult {
    InvalidArgument,
    InvalidMessage,
    TransportError,
    Empty,
    Message,
    Unknown(i32),
}

impl NativeResult {
    fn from_raw(value: i32) -> Self {
        match value {
            bindings::DMC2_ERROR_NATIVE_INVALID_ARGUMENT => Self::InvalidArgument,
            bindings::DMC2_ERROR_NATIVE_INVALID_MESSAGE => Self::InvalidMessage,
            bindings::DMC2_ERROR_NATIVE_TRANSPORT_ERROR => Self::TransportError,
            bindings::DMC2_ERROR_NATIVE_EMPTY => Self::Empty,
            bindings::DMC2_ERROR_NATIVE_MESSAGE => Self::Message,
            value => Self::Unknown(value),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::application) struct ErrorChannelFault {
    pub(in crate::application) transport: TransportStatus,
    reason: FaultReason,
}

impl fmt::Display for ErrorChannelFault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} (NML error {}, CMS status {})",
            self.reason, self.transport.nml_error, self.transport.cms_status
        )
    }
}

impl ErrorChannelFault {
    #[cfg(test)]
    pub(in crate::application) fn test_fault(transport: TransportStatus) -> Self {
        Self {
            transport,
            reason: FaultReason::Native(NativeResult::TransportError),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FaultReason {
    Native(NativeResult),
    InconsistentState,
    Decode(DecodeError),
}

impl fmt::Display for FaultReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(result) => write!(formatter, "native error-channel result {result:?}"),
            Self::InconsistentState => write!(formatter, "inconsistent error-channel state"),
            Self::Decode(error) => write!(formatter, "invalid error-channel message: {error}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
// Keeping the complete LinuxCNC error object inline makes the fault-reporting
// path independent of heap allocation after a message has been received.
#[allow(clippy::large_enum_variant)]
pub(in crate::application) enum ErrorChannelRead {
    Empty(TransportStatus),
    Message(TransportStatus, ErrorMessageRecord),
    Fault(ErrorChannelFault),
}

fn classify_native(result: i32, snapshot: RawErrorSnapshot, codes: PollCodes) -> ErrorChannelRead {
    let result = NativeResult::from_raw(result);
    let transport = TransportStatus {
        nml_error: snapshot.nml_error,
        cms_status: snapshot.cms_status,
    };
    match result {
        NativeResult::Empty
            if snapshot.message_type == 0
                && snapshot.object_size == 0
                && snapshot.abi_version == ERROR_MESSAGE_ABI_VERSION
                && snapshot.struct_size as usize == std::mem::size_of::<RawErrorSnapshot>()
                && snapshot.object.iter().all(|byte| *byte == 0)
                && transport.nml_error == codes.no_error
                && transport.cms_status == codes.cms_read_old =>
        {
            ErrorChannelRead::Empty(transport)
        }
        NativeResult::Message
            if snapshot.message_type > 0
                && snapshot.object_size > 0
                && transport.nml_error == codes.no_error
                && transport.cms_status == codes.cms_read_ok =>
        {
            match ErrorMessageRecord::decode(snapshot) {
                Ok(record) => ErrorChannelRead::Message(transport, record),
                Err(error) => ErrorChannelRead::Fault(ErrorChannelFault {
                    transport,
                    reason: FaultReason::Decode(error),
                }),
            }
        }
        NativeResult::Empty | NativeResult::Message => ErrorChannelRead::Fault(ErrorChannelFault {
            transport,
            reason: FaultReason::InconsistentState,
        }),
        result => ErrorChannelRead::Fault(ErrorChannelFault {
            transport,
            reason: FaultReason::Native(result),
        }),
    }
}

pub(in crate::application) struct ErrorChannel {
    native: *mut bindings::dmc2_error_channel,
}

impl ErrorChannel {
    pub(in crate::application) fn open(
        nml_file: &CString,
        codes: PollCodes,
    ) -> (Option<Self>, TransportStatus) {
        let mut nml_error = codes.invalid_configuration;
        let mut cms_status = codes.cms_status_not_set;
        let native = unsafe {
            bindings::dmc2_error_channel_open(nml_file.as_ptr(), &mut nml_error, &mut cms_status)
        };
        let transport = TransportStatus {
            nml_error,
            cms_status,
        };
        if native.is_null() {
            (None, transport)
        } else {
            (Some(Self { native }), transport)
        }
    }

    pub(in crate::application) fn read(&mut self, codes: PollCodes) -> ErrorChannelRead {
        let mut snapshot = RawErrorSnapshot::default();
        let result = unsafe { bindings::dmc2_error_channel_read(self.native, &mut snapshot) };
        classify_native(result, snapshot, codes)
    }
}

impl Drop for ErrorChannel {
    fn drop(&mut self) {
        unsafe { bindings::dmc2_error_channel_close(self.native) };
    }
}

pub(in crate::application) fn abi_version() -> u32 {
    unsafe { bindings::dmc2_error_message_abi_version() }
}

pub(in crate::application) fn snapshot_size() -> usize {
    unsafe { bindings::dmc2_error_message_snapshot_size() }
}

pub(in crate::application) fn copy_self_test() -> Result<u32, String> {
    let mut tested_message_types = 0;
    let mut failure_offset = usize::MAX;
    let result = unsafe {
        bindings::dmc2_error_message_copy_self_test(&mut tested_message_types, &mut failure_offset)
    };
    if result != 0 {
        return Err(format!(
            "native error-message copy failed in round {result} at byte {failure_offset}"
        ));
    }
    if tested_message_types as usize != ERROR_MESSAGE_CONTRACTS.len() {
        return Err(format!(
            "native error-message copy tested {tested_message_types} types, expected {}",
            ERROR_MESSAGE_CONTRACTS.len()
        ));
    }
    Ok(tested_message_types)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::ptr;

    use dmc2_linuxcnc_interface::{CMS_STATUS, NML_ERROR};

    use super::*;

    fn snapshot(codes: PollCodes) -> RawErrorSnapshot {
        let contract = ERROR_MESSAGE_CONTRACTS[0];
        let mut snapshot = RawErrorSnapshot {
            abi_version: ERROR_MESSAGE_ABI_VERSION,
            struct_size: std::mem::size_of::<RawErrorSnapshot>() as u32,
            message_type: contract.message_type as i32,
            nml_error: codes.no_error,
            cms_status: codes.cms_read_ok,
            object_size: contract.message_size as u32,
            object: [0; ERROR_OBJECT_CAPACITY],
        };
        snapshot.object[..4].copy_from_slice(&(contract.message_type as i32).to_ne_bytes());
        snapshot.object[8..16].copy_from_slice(&(contract.message_size as i64).to_ne_bytes());
        snapshot
    }

    #[test]
    fn native_abi_and_all_six_copy_paths_are_executed() {
        assert_eq!(abi_version(), ERROR_MESSAGE_ABI_VERSION);
        assert_eq!(snapshot_size(), std::mem::size_of::<RawErrorSnapshot>());
        assert_eq!(snapshot_size(), 304);
        assert_eq!(copy_self_test().unwrap(), 6);
    }

    #[test]
    fn native_channel_argument_guards_fail_closed_without_opening_nml() {
        unsafe { bindings::dmc2_error_message_snapshot_initialize(ptr::null_mut()) };
        let mut tested = 99;
        let mut failure_offset = 99;
        assert_eq!(
            unsafe {
                bindings::dmc2_error_message_copy_self_test(ptr::null_mut(), &mut failure_offset)
            },
            1
        );
        assert_eq!(
            unsafe { bindings::dmc2_error_message_copy_self_test(&mut tested, ptr::null_mut()) },
            1
        );

        let mut snapshot = RawErrorSnapshot::default();
        assert_eq!(
            unsafe { bindings::dmc2_error_channel_read(ptr::null_mut(), &mut snapshot) },
            bindings::DMC2_ERROR_NATIVE_TRANSPORT_ERROR
        );
        assert_eq!(snapshot.abi_version, ERROR_MESSAGE_ABI_VERSION);
        assert_eq!(
            snapshot.nml_error,
            PollCodes::required().invalid_configuration
        );
        assert_eq!(
            unsafe { bindings::dmc2_error_channel_read(ptr::null_mut(), ptr::null_mut()) },
            bindings::DMC2_ERROR_NATIVE_INVALID_ARGUMENT
        );
        let mut nml_error = 0;
        let mut cms_status = 0;
        assert!(unsafe {
            bindings::dmc2_error_channel_open(ptr::null(), &mut nml_error, &mut cms_status)
        }
        .is_null());
        assert_eq!(nml_error, PollCodes::required().invalid_configuration);
        assert_eq!(cms_status, PollCodes::required().cms_status_not_set);

        let empty = CString::new("").unwrap();
        nml_error = 0;
        cms_status = 0;
        assert!(unsafe {
            bindings::dmc2_error_channel_open(empty.as_ptr(), &mut nml_error, &mut cms_status)
        }
        .is_null());
        assert_eq!(nml_error, PollCodes::required().invalid_configuration);
        assert_eq!(cms_status, PollCodes::required().cms_status_not_set);

        let file = CString::new("unused.nml").unwrap();
        assert!(unsafe {
            bindings::dmc2_error_channel_open(file.as_ptr(), ptr::null_mut(), &mut cms_status)
        }
        .is_null());
        assert!(unsafe {
            bindings::dmc2_error_channel_open(file.as_ptr(), &mut nml_error, ptr::null_mut())
        }
        .is_null());
        unsafe { bindings::dmc2_error_channel_close(ptr::null_mut()) };
    }

    #[test]
    fn only_exact_linuxcnc_read_state_pairs_are_accepted() {
        let codes = PollCodes::required();
        let valid = snapshot(codes);
        assert!(matches!(
            classify_native(bindings::DMC2_ERROR_NATIVE_MESSAGE, valid, codes),
            ErrorChannelRead::Message(_, _)
        ));

        let empty = RawErrorSnapshot {
            abi_version: ERROR_MESSAGE_ABI_VERSION,
            struct_size: std::mem::size_of::<RawErrorSnapshot>() as u32,
            nml_error: codes.no_error,
            cms_status: codes.cms_read_old,
            ..RawErrorSnapshot::default()
        };
        assert_eq!(
            classify_native(bindings::DMC2_ERROR_NATIVE_EMPTY, empty, codes),
            ErrorChannelRead::Empty(TransportStatus {
                nml_error: codes.no_error,
                cms_status: codes.cms_read_old,
            })
        );

        let native_results = [
            i32::MIN,
            bindings::DMC2_ERROR_NATIVE_INVALID_ARGUMENT,
            bindings::DMC2_ERROR_NATIVE_INVALID_MESSAGE,
            bindings::DMC2_ERROR_NATIVE_TRANSPORT_ERROR,
            bindings::DMC2_ERROR_NATIVE_EMPTY,
            bindings::DMC2_ERROR_NATIVE_MESSAGE,
            2,
            i32::MAX,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        let message_types = [i32::MIN, -1, 0, valid.message_type, 77, i32::MAX];
        let nml_errors = NML_ERROR
            .codes
            .iter()
            .map(|entry| i32::try_from(entry.code).unwrap())
            .chain([i32::MIN, i32::MAX])
            .collect::<BTreeSet<_>>();
        let cms_statuses = CMS_STATUS
            .codes
            .iter()
            .map(|entry| i32::try_from(entry.code).unwrap())
            .chain([i32::MIN, i32::MAX])
            .collect::<BTreeSet<_>>();
        for result in native_results {
            for message_type in message_types {
                for nml_error in &nml_errors {
                    for cms_status in &cms_statuses {
                        let mut candidate = valid;
                        candidate.message_type = message_type;
                        candidate.nml_error = *nml_error;
                        candidate.cms_status = *cms_status;
                        if message_type <= 0 {
                            candidate.object_size = 0;
                            candidate.object.fill(0);
                        } else {
                            candidate.object[..4].copy_from_slice(&message_type.to_ne_bytes());
                        }
                        let exact_message = result == bindings::DMC2_ERROR_NATIVE_MESSAGE
                            && message_type > 0
                            && *nml_error == codes.no_error
                            && *cms_status == codes.cms_read_ok;
                        let exact_empty = result == bindings::DMC2_ERROR_NATIVE_EMPTY
                            && message_type == 0
                            && *nml_error == codes.no_error
                            && *cms_status == codes.cms_read_old;
                        let classified = classify_native(result, candidate, codes);
                        assert_eq!(
                            matches!(classified, ErrorChannelRead::Fault(_)),
                            !(exact_message || exact_empty),
                            "result={result} type={message_type} nml={nml_error} cms={cms_status}"
                        );
                    }
                }
            }
        }

        let mut malformed_empty = empty;
        malformed_empty.object[0] = 1;
        assert!(matches!(
            classify_native(bindings::DMC2_ERROR_NATIVE_EMPTY, malformed_empty, codes),
            ErrorChannelRead::Fault(_)
        ));
        malformed_empty = empty;
        malformed_empty.abi_version ^= 1;
        assert!(matches!(
            classify_native(bindings::DMC2_ERROR_NATIVE_EMPTY, malformed_empty, codes),
            ErrorChannelRead::Fault(_)
        ));
    }
}
