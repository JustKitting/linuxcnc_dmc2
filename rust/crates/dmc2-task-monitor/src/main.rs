use dmc2_diagnostics::RecoveryDisplay;

fn main() {
    if let Err(error) = dmc2_task_monitor::run() {
        eprintln!("dmc2-task-monitor: {}", RecoveryDisplay(&error));
        std::process::exit(1);
    }
}
