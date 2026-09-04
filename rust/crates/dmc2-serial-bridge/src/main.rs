mod application;

use dmc2_diagnostics::RecoveryDisplay;

fn main() {
    if let Err(error) = application::run() {
        eprintln!("dmc2-serial-bridge: {}", RecoveryDisplay(&error));
        std::process::exit(1);
    }
}
