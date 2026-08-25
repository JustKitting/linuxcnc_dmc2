use std::process::{Command, Output};

pub(crate) fn output(program: &str, arguments: &[&str]) -> Output {
    Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("failed to execute {program}: {error}"))
}

pub(crate) fn text(program: &str, arguments: &[&str]) -> String {
    let result = output(program, arguments);
    assert!(
        result.status.success(),
        "{program} failed with {}: {}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout)
        .unwrap_or_else(|error| panic!("{program} produced non-UTF-8 output: {error}"))
}
