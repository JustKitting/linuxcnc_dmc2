use std::ffi::CString;
use std::fmt;

use dmc2_linuxcnc_interface::{CodeDomain, CMS_STATUS, NML_ERROR};

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
            "{}: {}; evidence: reason={}, NML={}, CMS={}; action: {}",
            self.reason.identity(),
            self.reason.summary(),
            self.reason,
            SourceCodeDisplay {
                domain: NML_ERROR,
                raw: self.transport.nml_error,
                unknown_identity: "UNKNOWN_NML_ERROR",
            },
            SourceCodeDisplay {
                domain: CMS_STATUS,
                raw: self.transport.cms_status,
                unknown_identity: "UNKNOWN_CMS_STATUS",
            },
            self.reason.action(),
        )
    }
}

struct SourceCodeDisplay {
    domain: CodeDomain,
    raw: i32,
    unknown_identity: &'static str,
}

impl fmt::Display for SourceCodeDisplay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.domain.lookup(i64::from(self.raw)) {
            Some(identity) => write!(formatter, "{identity}(raw={})", self.raw),
            None => write!(formatter, "{}(raw={})", self.unknown_identity, self.raw),
        }
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

impl FaultReason {
    const fn identity(&self) -> &'static str {
        match self {
            Self::Native(NativeResult::InvalidArgument) => "ERROR_CHANNEL_NATIVE_INVALID_ARGUMENT",
            Self::Native(NativeResult::InvalidMessage) => "ERROR_CHANNEL_NATIVE_INVALID_MESSAGE",
            Self::Native(NativeResult::TransportError) => "ERROR_CHANNEL_NATIVE_TRANSPORT_ERROR",
            Self::Native(NativeResult::Empty) => "ERROR_CHANNEL_NATIVE_UNEXPECTED_EMPTY",
            Self::Native(NativeResult::Message) => "ERROR_CHANNEL_NATIVE_UNEXPECTED_MESSAGE",
            Self::Native(NativeResult::Unknown(_)) => "ERROR_CHANNEL_NATIVE_RESULT_UNKNOWN",
            Self::InconsistentState => "ERROR_CHANNEL_STATE_INCONSISTENT",
            Self::Decode(_) => "ERROR_CHANNEL_MESSAGE_DECODE_FAILED",
        }
    }

    const fn summary(&self) -> &'static str {
        match self {
            Self::Native(NativeResult::InvalidArgument) => {
                "the native LinuxCNC error-channel reader rejected its call arguments"
            }
            Self::Native(NativeResult::InvalidMessage) => {
                "the native reader rejected the LinuxCNC error-channel message contract"
            }
            Self::Native(NativeResult::TransportError) => {
                "the native reader reported a LinuxCNC NML/CMS transport failure"
            }
            Self::Native(NativeResult::Empty) => {
                "the native reader returned an empty result outside its valid empty-state contract"
            }
            Self::Native(NativeResult::Message) => {
                "the native reader returned a message result outside its valid message-state contract"
            }
            Self::Native(NativeResult::Unknown(_)) => {
                "the native reader returned a result absent from its compiled result catalog"
            }
            Self::InconsistentState => {
                "the native result, copied object, and NML/CMS transport fields disagree"
            }
            Self::Decode(_) => {
                "a copied LinuxCNC error-channel object violates its verified 2.9.10 layout contract"
            }
        }
    }

    const fn action(&self) -> &'static str {
        match self {
            Self::Native(NativeResult::InvalidArgument) => {
                "stop the monitor and correct the native reader invocation"
            }
            Self::Native(NativeResult::InvalidMessage) | Self::Decode(_) => {
                "preserve the raw object and rebuild the monitor against the pinned LinuxCNC 2.9.10 interface"
            }
            Self::Native(NativeResult::TransportError) => {
                "use the named NML and CMS states below to restore the LinuxCNC error channel"
            }
            Self::Native(NativeResult::Empty)
            | Self::Native(NativeResult::Message)
            | Self::InconsistentState => {
                "preserve the raw channel state and restart only after correcting the producer/reader mismatch"
            }
            Self::Native(NativeResult::Unknown(_)) => {
                "retain the raw native result and correct the binary/source-version mismatch"
            }
        }
    }
}

impl fmt::Display for FaultReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(NativeResult::Unknown(raw)) => {
                write!(formatter, "native_result=UNKNOWN(raw={raw})")
            }
            Self::Native(result) => write!(formatter, "native_result={result:?}"),
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
