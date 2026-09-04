mod application;
mod diagnostics;
mod snapshot;

pub mod heartbeat;

pub use application::nml::RequiredCodeError;

use dmc2_diagnostics::RecoveryClassified;

pub fn run() -> Result<(), impl std::fmt::Display + RecoveryClassified> {
    application::run()
}
