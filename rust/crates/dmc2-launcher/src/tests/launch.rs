use std::ffi::OsString;
use std::io::{self, Write};

use super::support::{MockPlatform, RunOutcome};
use crate::cli::Mode;
use crate::error::{Error, OwnerMatch};
use crate::launch::{execute, prepare, Action};
use crate::owner::{assert_exclusive, CONFLICT_PATTERNS};

fn clear_nominal_validation(platform: &MockPlatform) {
    platform.runs.borrow_mut().clear();
}

#[test]
fn owner_probe_accepts_only_clean_pgrep_no_match_results() {
    let (platform, _) = MockPlatform::nominal();
    clear_nominal_validation(&platform);
    platform.push_clear_owners();
    assert_exclusive(&platform).unwrap();
    platform.assert_runs_consumed();
}

#[test]
fn every_owner_pattern_and_multiple_owners_are_reported() {
    for (selected, &selected_pattern) in CONFLICT_PATTERNS.iter().enumerate() {
        let (platform, _) = MockPlatform::nominal();
        clear_nominal_validation(&platform);
        for (index, pattern) in CONFLICT_PATTERNS.iter().enumerate() {
            let status = if index == selected { Some(0) } else { Some(1) };
            platform.push_run(
                "/usr/bin/pgrep",
                &["-f", pattern],
                MockPlatform::output(status, if status == Some(0) { b"123\n" } else { b"" }, b""),
            );
        }
        assert_eq!(
            assert_exclusive(&platform),
            Err(Error::OwnerConflict(vec![OwnerMatch {
                pattern: selected_pattern,
                stdout: b"123\n".to_vec(),
                stderr: Vec::new(),
            }]))
        );
    }

    let (platform, _) = MockPlatform::nominal();
    clear_nominal_validation(&platform);
    for pattern in CONFLICT_PATTERNS {
        platform.push_run(
            "/usr/bin/pgrep",
            &["-f", pattern],
            MockPlatform::output(Some(0), b"123\n", b""),
        );
    }
    assert_eq!(
        assert_exclusive(&platform),
        Err(Error::OwnerConflict(
            CONFLICT_PATTERNS
                .iter()
                .map(|pattern| OwnerMatch {
                    pattern,
                    stdout: b"123\n".to_vec(),
                    stderr: Vec::new(),
                })
                .collect()
        ))
    );
}

#[test]
fn matching_owner_stdout_and_stderr_are_preserved_as_evidence() {
    let (platform, _) = MockPlatform::nominal();
    clear_nominal_validation(&platform);
    platform.push_run(
        "/usr/bin/pgrep",
        &["-f", CONFLICT_PATTERNS[0]],
        MockPlatform::output(Some(0), b"pid-bytes\n", b"probe-warning"),
    );
    for pattern in &CONFLICT_PATTERNS[1..] {
        platform.push_run(
            "/usr/bin/pgrep",
            &["-f", pattern],
            MockPlatform::output(Some(1), b"", b""),
        );
    }
    assert_eq!(
        assert_exclusive(&platform),
        Err(Error::OwnerConflict(vec![OwnerMatch {
            pattern: CONFLICT_PATTERNS[0],
            stdout: b"pid-bytes\n".to_vec(),
            stderr: b"probe-warning".to_vec(),
        }]))
    );
}

#[test]
fn every_abnormal_owner_probe_completion_fails_closed() {
    for status in [Some(1), Some(2), Some(126), Some(127), Some(255), None] {
        for (stdout, stderr) in [
            (&b"unexpected"[..], &b""[..]),
            (&b""[..], &b"unexpected"[..]),
        ] {
            let (platform, _) = MockPlatform::nominal();
            clear_nominal_validation(&platform);
            platform.push_run(
                "/usr/bin/pgrep",
                &["-f", CONFLICT_PATTERNS[0]],
                MockPlatform::output(status, stdout, stderr),
            );
            let result = assert_exclusive(&platform);
            if status == Some(0) {
                unreachable!();
            }
            assert!(
                matches!(result, Err(Error::OwnerProbe { status: observed, .. }) if observed == status)
            );
        }
    }
}

#[test]
fn owner_probe_missing_executable_and_spawn_failure_are_preserved() {
    let (mut platform, _) = MockPlatform::nominal();
    clear_nominal_validation(&platform);
    platform.executables.remove("pgrep");
    assert_eq!(
        assert_exclusive(&platform),
        Err(Error::ExecutableUnavailable("pgrep"))
    );

    let (platform, _) = MockPlatform::nominal();
    clear_nominal_validation(&platform);
    platform.push_run(
        "/usr/bin/pgrep",
        &["-f", CONFLICT_PATTERNS[0]],
        RunOutcome::Failure(io::ErrorKind::PermissionDenied),
    );
    assert!(matches!(
        assert_exclusive(&platform),
        Err(Error::OperatingSystem {
            operation: "execute process",
            ..
        })
    ));
}

#[test]
fn validation_mode_never_probes_owners_or_builds_a_live_command() {
    let (platform, layout) = MockPlatform::nominal();
    let plan = prepare(&platform, &layout, Mode::Validate).unwrap();
    assert_eq!(plan.action, Action::Validate);
    platform.assert_runs_consumed();
}

#[test]
fn direct_plan_is_the_exact_linuxcnc_process_replacement() {
    let (platform, layout) = MockPlatform::nominal();
    platform.push_clear_owners();
    let plan = prepare(&platform, &layout, Mode::Direct).unwrap();
    let Action::Replace(command) = plan.action else {
        panic!("expected direct replacement");
    };
    assert_eq!(command.program.to_str(), Some("/usr/bin/linuxcnc"));
    assert_eq!(
        command.arguments,
        vec![
            OsString::from("-r"),
            layout.project.join("live/dmc2.ini").into_os_string(),
        ]
    );
    assert_eq!(command.working_directory, Some(layout.project.join("live")));
    assert_eq!(
        command
            .environment
            .get(&OsString::from("LINUXCNC_FORCE_REALTIME")),
        Some(&OsString::from("1"))
    );
    platform.assert_runs_consumed();
}

#[test]
fn persistent_plan_uses_only_the_deployed_rust_launcher() {
    let (platform, layout) = MockPlatform::nominal();
    platform.push_clear_owners();
    let plan = prepare(&platform, &layout, Mode::Persistent).unwrap();
    let Action::Persistent(command) = plan.action else {
        panic!("expected persistent command");
    };
    assert_eq!(command.program.to_str(), Some("/usr/bin/systemd-run"));
    assert!(command.arguments.contains(&OsString::from("--quiet")));
    assert_eq!(command.arguments.last(), Some(&OsString::from("--live")));
    assert!(command.arguments.contains(
        &layout
            .project
            .join("native/bin/dmc2-linuxcnc")
            .into_os_string()
    ));
    assert!(!command.arguments.iter().any(|argument| {
        let text = argument.to_string_lossy();
        text.contains("python") || text.contains("launch_live.py")
    }));
    platform.assert_runs_consumed();
}

#[test]
fn missing_systemd_run_is_rejected_only_for_persistent_mode() {
    let (mut platform, layout) = MockPlatform::nominal();
    platform.executables.remove("systemd-run");
    platform.push_clear_owners();
    assert_eq!(
        prepare(&platform, &layout, Mode::Persistent),
        Err(Error::ExecutableUnavailable("systemd-run"))
    );
}

#[test]
fn prepare_preserves_validation_and_owner_failures_before_building_an_action() {
    let (platform, layout) = MockPlatform::nominal();
    let required = layout.project.join("live/dmc2.ini");
    platform.files.borrow_mut().remove(&required);
    assert_eq!(
        prepare(&platform, &layout, Mode::Validate),
        Err(Error::NotRegularFile(required))
    );

    let (mut platform, layout) = MockPlatform::nominal();
    platform.executables.remove("pgrep");
    assert_eq!(
        prepare(&platform, &layout, Mode::Direct),
        Err(Error::ExecutableUnavailable("pgrep"))
    );

    let (mut platform, layout) = MockPlatform::nominal();
    platform.executables.remove("pgrep");
    assert_eq!(
        prepare(&platform, &layout, Mode::Persistent),
        Err(Error::ExecutableUnavailable("pgrep"))
    );
}

#[test]
fn execute_validation_writes_passes_and_explicit_no_hardware_result() {
    let (platform, _) = MockPlatform::nominal();
    let mut output = Vec::new();
    let result = execute(
        &platform,
        crate::launch::Plan {
            action: Action::Validate,
        },
        &mut output,
    );
    assert_eq!(result, Ok(0));
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("VALIDATION ONLY"));
    assert_eq!(
        output.matches("PASS:").count(),
        crate::validation::PASSES.len()
    );
}

#[test]
fn persistent_execution_preserves_service_output_and_all_failure_states() {
    for status in [Some(0), Some(1), Some(255), None] {
        let (platform, _) = MockPlatform::nominal();
        clear_nominal_validation(&platform);
        let command = crate::platform::CommandSpec::new("/usr/bin/systemd-run");
        platform.push_run(
            "/usr/bin/systemd-run",
            &[],
            MockPlatform::output(
                status,
                b"service bytes",
                if status == Some(0) {
                    b""
                } else {
                    b"service error"
                },
            ),
        );
        let mut output = Vec::new();
        let result = execute(
            &platform,
            crate::launch::Plan {
                action: Action::Persistent(command),
            },
            &mut output,
        );
        if status == Some(0) {
            assert_eq!(result, Ok(0));
            assert!(output
                .windows(b"service bytes".len())
                .any(|part| part == b"service bytes"));
        } else {
            assert!(
                matches!(result, Err(Error::ProcessFailed { status: observed, .. }) if observed == status)
            );
        }
    }

    let (platform, _) = MockPlatform::nominal();
    clear_nominal_validation(&platform);
    platform.push_run(
        "/usr/bin/systemd-run",
        &[],
        RunOutcome::Failure(io::ErrorKind::NotFound),
    );
    let result = execute(
        &platform,
        crate::launch::Plan {
            action: Action::Persistent(crate::platform::CommandSpec::new("/usr/bin/systemd-run")),
        },
        &mut Vec::new(),
    );
    assert!(matches!(result, Err(Error::OperatingSystem { .. })));
}

#[test]
fn successful_service_exit_with_stderr_is_rejected_without_discarding_bytes() {
    let (platform, _) = MockPlatform::nominal();
    clear_nominal_validation(&platform);
    platform.push_run(
        "/usr/bin/systemd-run",
        &[],
        MockPlatform::output(Some(0), b"service stdout", b"service stderr"),
    );
    let result = execute(
        &platform,
        crate::launch::Plan {
            action: Action::Persistent(crate::platform::CommandSpec::new("/usr/bin/systemd-run")),
        },
        &mut Vec::new(),
    );
    assert_eq!(
        result,
        Err(Error::ProcessFailed {
            program: std::path::PathBuf::from("/usr/bin/systemd-run"),
            status: Some(0),
            stdout: b"service stdout".to_vec(),
            stderr: b"service stderr".to_vec(),
        })
    );
}

#[test]
fn direct_execution_records_os_failure_and_impossible_return() {
    let (platform, _) = MockPlatform::nominal();
    let command = crate::platform::CommandSpec::new("/usr/bin/linuxcnc");
    let result = execute(
        &platform,
        crate::launch::Plan {
            action: Action::Replace(command.clone()),
        },
        &mut Vec::new(),
    );
    assert!(matches!(
        result,
        Err(Error::OperatingSystem {
            operation: "replace process",
            ..
        })
    ));
    assert_eq!(
        platform.replacements.borrow().as_slice(),
        &[command.clone()]
    );

    *platform.replacement_failure.borrow_mut() = None;
    let result = execute(
        &platform,
        crate::launch::Plan {
            action: Action::Replace(command),
        },
        &mut Vec::new(),
    );
    assert_eq!(result, Err(Error::ExecReturned));
}

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::from(io::ErrorKind::BrokenPipe))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct FailAfter {
    remaining: usize,
}

impl Write for FailAfter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.len() > self.remaining {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        self.remaining -= buffer.len();
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn pass_output_size() -> usize {
    crate::validation::PASSES
        .iter()
        .map(|pass| format!("PASS: {pass}\n").len())
        .sum()
}

#[test]
fn operator_output_failure_is_never_discarded() {
    let (platform, _) = MockPlatform::nominal();
    let result = execute(
        &platform,
        crate::launch::Plan {
            action: Action::Validate,
        },
        &mut FailingWriter,
    );
    assert!(matches!(
        result,
        Err(Error::OperatingSystem {
            operation: "write launcher output",
            ..
        })
    ));
}

#[test]
fn every_post_validation_output_failure_has_its_exact_context() {
    let (platform, _) = MockPlatform::nominal();
    let result = execute(
        &platform,
        crate::launch::Plan {
            action: Action::Validate,
        },
        &mut FailAfter {
            remaining: pass_output_size(),
        },
    );
    assert!(matches!(
        result,
        Err(Error::OperatingSystem {
            operation: "write launcher output",
            ..
        })
    ));

    for (service_output, allowance, expected_operation) in [
        (&b"bytes"[..], pass_output_size(), "write service output"),
        (
            &b"bytes"[..],
            pass_output_size() + b"bytes".len(),
            "write service output",
        ),
        (
            &b"bytes\n"[..],
            pass_output_size() + b"bytes\n".len(),
            "write launcher output",
        ),
    ] {
        let (platform, _) = MockPlatform::nominal();
        clear_nominal_validation(&platform);
        platform.push_run(
            "/usr/bin/systemd-run",
            &[],
            MockPlatform::output(Some(0), service_output, b""),
        );
        let result = execute(
            &platform,
            crate::launch::Plan {
                action: Action::Persistent(crate::platform::CommandSpec::new(
                    "/usr/bin/systemd-run",
                )),
            },
            &mut FailAfter {
                remaining: allowance,
            },
        );
        assert!(matches!(
            result,
            Err(Error::OperatingSystem { operation, .. }) if operation == expected_operation
        ));
    }
}

#[test]
fn successful_service_output_handles_empty_terminated_and_unterminated_bytes() {
    for service_output in [&b""[..], &b"terminated\n"[..], &b"unterminated"[..]] {
        let (platform, _) = MockPlatform::nominal();
        clear_nominal_validation(&platform);
        platform.push_run(
            "/usr/bin/systemd-run",
            &[],
            MockPlatform::output(Some(0), service_output, b""),
        );
        let mut output = Vec::new();
        assert_eq!(
            execute(
                &platform,
                crate::launch::Plan {
                    action: Action::Persistent(crate::platform::CommandSpec::new(
                        "/usr/bin/systemd-run",
                    )),
                },
                &mut output,
            ),
            Ok(0)
        );
        if !service_output.is_empty() {
            assert!(output
                .windows(service_output.len())
                .any(|part| part == service_output));
        }
    }
}

#[test]
fn top_level_help_never_discovers_project_or_runs_a_process() {
    let (mut platform, _) = MockPlatform::nominal();
    platform.executable_failure = Some(io::ErrorKind::PermissionDenied);
    clear_nominal_validation(&platform);
    let mut output = Vec::new();
    assert_eq!(
        crate::run(&platform, &[OsString::from("--help")], &mut output),
        Ok(0)
    );
    assert!(String::from_utf8(output).unwrap().contains("usage:"));
}

#[test]
fn top_level_validation_traverses_discovery_validation_and_no_hardware_execution() {
    let (platform, _) = MockPlatform::nominal();
    let mut output = Vec::new();
    assert_eq!(crate::run(&platform, &[], &mut output), Ok(0));
    assert!(String::from_utf8(output)
        .unwrap()
        .contains("VALIDATION ONLY"));
    platform.assert_runs_consumed();
    assert!(platform.replacements.borrow().is_empty());
}

#[test]
fn top_level_direct_mode_reaches_only_the_exact_process_replacement() {
    let (platform, _) = MockPlatform::nominal();
    platform.push_clear_owners();
    let result = crate::run(&platform, &[OsString::from("--live")], &mut Vec::new());
    assert!(matches!(
        result,
        Err(Error::OperatingSystem {
            operation: "replace process",
            ..
        })
    ));
    platform.assert_runs_consumed();
    assert_eq!(platform.replacements.borrow().len(), 1);
}

#[test]
fn top_level_usage_and_discovery_failures_stop_before_validation() {
    let (platform, _) = MockPlatform::nominal();
    platform.runs.borrow_mut().clear();
    assert!(matches!(
        crate::run(
            &platform,
            &[OsString::from("--unsupported")],
            &mut Vec::new()
        ),
        Err(Error::Usage(_))
    ));

    let (mut platform, _) = MockPlatform::nominal();
    platform.runs.borrow_mut().clear();
    platform.executable = "/outside/project/dmc2-linuxcnc".into();
    assert!(matches!(
        crate::run(&platform, &[], &mut Vec::new()),
        Err(Error::ProjectRootNotFound(_))
    ));
}

#[test]
fn top_level_prepare_failure_is_preserved_before_execution() {
    let (mut platform, _) = MockPlatform::nominal();
    platform.executables.remove("pgrep");
    assert_eq!(
        crate::run(&platform, &[OsString::from("--live")], &mut Vec::new()),
        Err(Error::ExecutableUnavailable("pgrep"))
    );
    assert!(platform.replacements.borrow().is_empty());
}

#[test]
fn top_level_help_output_failure_is_preserved() {
    let (platform, _) = MockPlatform::nominal();
    platform.runs.borrow_mut().clear();
    assert!(matches!(
        crate::run(&platform, &[OsString::from("--help")], &mut FailingWriter),
        Err(Error::OperatingSystem {
            operation: "write launcher output",
            ..
        })
    ));
}
