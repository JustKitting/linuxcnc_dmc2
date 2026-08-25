use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_dmc2-linuxcnc");

#[test]
fn compiled_entrypoint_help_returns_success_without_project_or_hardware_access() {
    let output = Command::new(BINARY).arg("--help").output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        output.stdout,
        b"usage: dmc2-linuxcnc [--live [--persistent]]\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn compiled_entrypoint_reports_usage_failure_on_stderr_with_exit_two() {
    let output = Command::new(BINARY)
        .arg("--not-a-real-option")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(output
        .stderr
        .starts_with(b"LIVE LAUNCH REFUSED: usage: dmc2-linuxcnc"));
}
