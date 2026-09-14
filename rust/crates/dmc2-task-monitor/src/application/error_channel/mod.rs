mod cause;
mod journal;
mod native;
mod record;

pub(super) use journal::ErrorJournal;
pub(super) use native::{
    abi_version, snapshot_size, ErrorChannel, ErrorChannelFault, ErrorChannelRead,
    RawErrorSnapshot, ERROR_MESSAGE_ABI_VERSION,
};
pub(super) use record::ErrorMessageRecord;
