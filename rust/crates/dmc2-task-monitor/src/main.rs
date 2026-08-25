mod application;
mod diagnostics;
mod snapshot;

fn main() {
    if let Err(error) = application::run() {
        eprintln!("dmc2-task-monitor: {error}");
        std::process::exit(1);
    }
}
