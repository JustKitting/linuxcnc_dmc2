use std::ffi::{c_int, CString};
use std::fmt;

use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};
use dmc2_linuxcnc_interface::{CodeDomain, CMS_STATUS, EMC_NML_MESSAGE_TYPE, NML_ERROR};

use crate::snapshot::{
    derive_axis_stopped, dmc2_task_status_close, dmc2_task_status_copy, dmc2_task_status_observe,
    dmc2_task_status_open, dmc2_task_status_snapshot_abi_version, dmc2_task_status_snapshot_size,
    NativeSnapshot, NativeTaskStatusChannel, DMC2_TASK_STATUS_NATIVE_OK,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PollDisposition {
    Snapshot,
    WaitingForFirstStatus,
    Fault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PollCodes {
    pub(crate) no_error: i32,
    pub(crate) invalid_configuration: i32,
    pub(crate) invalid_message: i32,
    pub(crate) status_message_type: i32,
    pub(crate) cms_status_not_set: i32,
    pub(crate) cms_read_old: i32,
    pub(crate) cms_read_ok: i32,
}

impl PollCodes {
    pub(crate) fn required() -> Result<Self, RequiredCodeError> {
        Ok(Self {
            no_error: required_code(NML_ERROR, "NML_NO_ERROR")?,
            invalid_configuration: required_code(NML_ERROR, "NML_INVALID_CONFIGURATION")?,
            invalid_message: required_code(NML_ERROR, "NML_INVALID_MESSAGE_ERROR")?,
            status_message_type: required_code(EMC_NML_MESSAGE_TYPE, "EMC_STAT_TYPE")?,
            cms_status_not_set: required_code(CMS_STATUS, "CMS_STATUS_NOT_SET")?,
            cms_read_old: required_code(CMS_STATUS, "CMS_READ_OLD")?,
            cms_read_ok: required_code(CMS_STATUS, "CMS_READ_OK")?,
        })
    }
}

/// A required value is absent from, or cannot be represented from, the pinned
/// LinuxCNC interface catalog. This is an operator-facing typed startup error,
/// never a process panic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequiredCodeError {
    Missing {
        domain: &'static str,
        name: &'static str,
    },
    OutOfRange {
        domain: &'static str,
        name: &'static str,
        value: i64,
    },
}

impl RequiredCodeError {
    pub const fn identity(self) -> &'static str {
        match self {
            Self::Missing { .. } => "LINUXCNC_REQUIRED_CODE_MISSING",
            Self::OutOfRange { .. } => "LINUXCNC_REQUIRED_CODE_OUT_OF_RANGE",
        }
    }
}

impl fmt::Display for RequiredCodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { domain, name } => write!(
                formatter,
                "{}: domain={domain} name={name}; cause: the pinned LinuxCNC interface catalog does not contain a required value; action: reinstall the matching binaries and relaunch DMC2 LinuxCNC",
                self.identity()
            ),
            Self::OutOfRange {
                domain,
                name,
                value,
            } => write!(
                formatter,
                "{}: domain={domain} name={name} value={value}; cause: a required LinuxCNC interface value cannot be represented by the native i32 ABI; action: reinstall the matching binaries and relaunch DMC2 LinuxCNC",
                self.identity()
            ),
        }
    }
}

impl RecoveryClassified for RequiredCodeError {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::Missing { .. } | Self::OutOfRange { .. } => RecoveryClass::RelaunchApplication,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TransportStatus {
    pub(crate) nml_error: i32,
    pub(crate) cms_status: i32,
}

impl TransportStatus {
    pub(crate) fn healthy_after_open(self, codes: PollCodes) -> bool {
        // CMS::open() explicitly initializes status to CMS_STATUS_NOT_SET.
        // A read, write, clear, closed, unknown, or error state here is not a
        // successful untouched NML channel, even when its numeric value is
        // nonnegative.
        self.nml_error == codes.no_error && self.cms_status == codes.cms_status_not_set
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PollOutcome {
    pub(crate) disposition: PollDisposition,
    pub(crate) transport: TransportStatus,
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

fn required_code(domain: CodeDomain, name: &'static str) -> Result<i32, RequiredCodeError> {
    let value = domain
        .codes
        .iter()
        .find(|entry| entry.name == name)
        .ok_or(RequiredCodeError::Missing {
            domain: domain.name,
            name,
        })?
        .code;
    value.try_into().map_err(|_| RequiredCodeError::OutOfRange {
        domain: domain.name,
        name,
        value,
    })
}

pub(super) fn snapshot_abi_version() -> u32 {
    unsafe { dmc2_task_status_snapshot_abi_version() }
}

pub(super) fn snapshot_size() -> usize {
    unsafe { dmc2_task_status_snapshot_size() }
}

pub(crate) struct StatusChannel {
    native: *mut NativeTaskStatusChannel,
    received_status: bool,
}

impl StatusChannel {
    pub(crate) fn open(nml_file: &CString, codes: PollCodes) -> (Option<Self>, TransportStatus) {
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

    pub(crate) fn poll(&mut self, snapshot: &mut NativeSnapshot, codes: PollCodes) -> PollOutcome {
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
