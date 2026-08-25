//! HAL adapter for coherent pendant snapshots.

mod pins;
mod publisher;
mod registration;

pub(super) use publisher::HalPublisher;
