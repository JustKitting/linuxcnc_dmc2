fn main() {
    if let Err(error) = dmc2_task_monitor::run() {
        eprintln!("dmc2-task-monitor: {error}");
        std::process::exit(1);
    }
}
