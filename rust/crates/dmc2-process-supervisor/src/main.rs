use std::process::ExitCode;

use dmc2_diagnostics::RecoveryDisplay;

fn main() -> ExitCode {
    match dmc2_process_supervisor::run(std::env::args_os().skip(1)) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("dmc2-process-supervisor: {}", RecoveryDisplay(&error));
            ExitCode::from(dmc2_process_supervisor::TRACKING_FAILURE_EXIT_CODE)
        }
    }
}
