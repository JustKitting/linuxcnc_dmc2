//! HAL adapter for coherent pendant snapshots.

mod pins;
mod publisher;
mod registration;

pub use publisher::HalPublisher;
pub use registration::RegistrationError;
