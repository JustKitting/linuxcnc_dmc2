mod application;
mod diagnostics;
mod snapshot;

pub mod heartbeat;

pub fn run() -> Result<(), impl std::fmt::Display> {
    application::run()
}
