mod cli;
mod diagnostic_state;
mod error_channel;
mod hal;
mod nml;
mod program_validation;
mod runtime;

use std::mem;

use crate::snapshot::{NativeSnapshot, SNAPSHOT_ABI_VERSION};

use self::cli::arguments;

pub(super) fn run() -> Result<(), String> {
    let args = arguments()?;
    let native_abi = nml::snapshot_abi_version();
    let native_size = nml::snapshot_size();
    if native_abi != SNAPSHOT_ABI_VERSION || native_size != mem::size_of::<NativeSnapshot>() {
        return Err(format!(
            "native snapshot ABI mismatch: C++ version=0x{native_abi:08x} size={native_size}, Rust version=0x{SNAPSHOT_ABI_VERSION:08x} size={}",
            mem::size_of::<NativeSnapshot>()
        ));
    }
    let native_error_abi = error_channel::abi_version();
    let native_error_size = error_channel::snapshot_size();
    if native_error_abi != error_channel::ERROR_MESSAGE_ABI_VERSION
        || native_error_size != mem::size_of::<error_channel::RawErrorSnapshot>()
    {
        return Err(format!(
            "native error-message ABI mismatch: C++ version=0x{native_error_abi:08x} size={native_error_size}, Rust version=0x{:08x} size={}",
            error_channel::ERROR_MESSAGE_ABI_VERSION,
            mem::size_of::<error_channel::RawErrorSnapshot>()
        ));
    }
    if args.validate {
        return program_validation::run(args.validation_json);
    }
    runtime::run(args)
}
