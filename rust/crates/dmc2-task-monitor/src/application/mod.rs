mod cli;
mod diagnostic_journal;
mod diagnostic_state;
mod error;
mod error_channel;
mod hal;
mod journal_error;
pub(crate) mod nml;
mod probe;
mod runtime;

use std::mem;

use dmc2_diagnostics::RecoveryClassified;

use crate::snapshot::{NativeSnapshot, SNAPSHOT_ABI_VERSION};

use self::cli::arguments;
use self::error::ApplicationError;

pub(crate) fn run() -> Result<(), impl std::fmt::Display + RecoveryClassified> {
    let args = arguments()?;
    let native_abi = nml::snapshot_abi_version();
    let native_size = nml::snapshot_size();
    if native_abi != SNAPSHOT_ABI_VERSION || native_size != mem::size_of::<NativeSnapshot>() {
        return Err(ApplicationError::TaskStatusAbiMismatch {
            native_version: native_abi,
            native_size,
            rust_version: SNAPSHOT_ABI_VERSION,
            rust_size: mem::size_of::<NativeSnapshot>(),
        });
    }
    let native_error_abi = error_channel::abi_version();
    let native_error_size = error_channel::snapshot_size();
    if native_error_abi != error_channel::ERROR_MESSAGE_ABI_VERSION
        || native_error_size != mem::size_of::<error_channel::RawErrorSnapshot>()
    {
        return Err(ApplicationError::ErrorMessageAbiMismatch {
            native_version: native_error_abi,
            native_size: native_error_size,
            rust_version: error_channel::ERROR_MESSAGE_ABI_VERSION,
            rust_size: mem::size_of::<error_channel::RawErrorSnapshot>(),
        });
    }
    runtime::run(args).map_err(Into::into)
}
