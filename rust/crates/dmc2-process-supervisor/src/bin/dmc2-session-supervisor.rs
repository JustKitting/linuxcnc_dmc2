use std::process::ExitCode;

fn main() -> ExitCode {
    match dmc2_process_supervisor::run_session(std::env::args_os().skip(1)) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("dmc2-session-supervisor: {error}");
            ExitCode::from(dmc2_process_supervisor::TRACKING_FAILURE_EXIT_CODE)
        }
    }
}
