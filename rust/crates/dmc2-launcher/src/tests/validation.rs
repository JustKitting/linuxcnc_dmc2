use std::io;

use crate::error::Error;
use crate::integrity::deployments;
use crate::validation::{validate, EXPECTED_LINUXCNC_VERSION, EXPECTED_TASK_MONITOR_VALIDATION};

use super::support::{FileEntry, MockPlatform, RunOutcome};

fn reset_runs(platform: &MockPlatform) {
    platform.runs.borrow_mut().clear();
}

#[test]
fn nominal_validation_consumes_the_exact_version_and_program_validation() {
    let (platform, layout) = MockPlatform::nominal();
    validate(&platform, &layout).unwrap();
    platform.assert_runs_consumed();
}

#[test]
fn validation_preserves_embedded_input_and_deployment_failures() {
    let (platform, layout) = MockPlatform::nominal();
    let embedded = layout.project.join("live/dmc2.ini");
    platform.files.borrow_mut().remove(&embedded);
    assert_eq!(
        validate(&platform, &layout),
        Err(Error::NotRegularFile(embedded))
    );

    let (platform, layout) = MockPlatform::nominal();
    let (deployed, staged) = deployments(&layout)[0].clone();
    platform
        .files
        .borrow_mut()
        .insert(deployed.clone(), FileEntry::Regular(b"mismatch".to_vec()));
    assert_eq!(
        validate(&platform, &layout),
        Err(Error::DeploymentMismatch { deployed, staged })
    );
}

#[test]
fn every_required_executable_is_mandatory() {
    for missing in ["linuxcnc_var", "linuxcnc"] {
        let (mut platform, layout) = MockPlatform::nominal();
        platform.executables.remove(missing);
        let result = validate(&platform, &layout);
        assert_eq!(result, Err(Error::ExecutableUnavailable(missing)));
    }

    for failed in ["linuxcnc_var", "linuxcnc"] {
        let (mut platform, layout) = MockPlatform::nominal();
        platform
            .executable_failures
            .insert(failed.to_owned(), io::ErrorKind::PermissionDenied);
        assert!(matches!(
            validate(&platform, &layout),
            Err(Error::OperatingSystem {
                operation: "locate executable",
                ..
            })
        ));
    }
}

#[test]
fn every_process_completion_category_is_handled_for_version_and_program_validation() {
    for status in [Some(1), Some(2), Some(126), Some(127), Some(255), None] {
        let (platform, layout) = MockPlatform::nominal();
        reset_runs(&platform);
        platform.push_run(
            "/usr/bin/linuxcnc_var",
            &["LINUXCNCVERSION"],
            MockPlatform::output(status, b"version-out", b"version-err"),
        );
        assert!(matches!(
            validate(&platform, &layout),
            Err(Error::ProcessFailed { status: observed, .. }) if observed == status
        ));

        let (platform, layout) = MockPlatform::nominal();
        reset_runs(&platform);
        platform.push_run(
            "/usr/bin/linuxcnc_var",
            &["LINUXCNCVERSION"],
            MockPlatform::output(Some(0), EXPECTED_LINUXCNC_VERSION, b""),
        );
        platform.push_run(
            layout.project.join("native/bin/dmc2-task-monitor"),
            &["--validate"],
            MockPlatform::output(status, b"audit-out", b"audit-err"),
        );
        assert!(matches!(
            validate(&platform, &layout),
            Err(Error::ProcessFailed { status: observed, .. }) if observed == status
        ));
    }
}

#[test]
fn version_requires_exact_stdout_and_empty_stderr() {
    for (stdout, stderr) in [
        (&b"2.9.9\n"[..], &b""[..]),
        (&b"2.9.10"[..], &b""[..]),
        (&b"2.9.10\r\n"[..], &b""[..]),
        (&b"2.9.10\n"[..], &b"warning"[..]),
        (&b"2.9.10\n\0"[..], &b""[..]),
        (&b"\xff"[..], &b""[..]),
    ] {
        let (platform, layout) = MockPlatform::nominal();
        reset_runs(&platform);
        platform.push_run(
            "/usr/bin/linuxcnc_var",
            &["LINUXCNCVERSION"],
            MockPlatform::output(Some(0), stdout, stderr),
        );
        assert!(matches!(
            validate(&platform, &layout),
            Err(Error::LinuxCncVersion { .. })
        ));
    }
}

#[test]
fn task_monitor_validation_requires_every_exact_byte_and_empty_stderr() {
    let mut changed = EXPECTED_TASK_MONITOR_VALIDATION.to_vec();
    changed[0] ^= 1;
    for (stdout, stderr) in [
        (changed.as_slice(), &b""[..]),
        (
            &EXPECTED_TASK_MONITOR_VALIDATION[..EXPECTED_TASK_MONITOR_VALIDATION.len() - 1],
            &b""[..],
        ),
        (EXPECTED_TASK_MONITOR_VALIDATION, &b"warning"[..]),
    ] {
        let (platform, layout) = MockPlatform::nominal();
        reset_runs(&platform);
        platform.push_nominal_validation(&layout);
        let mut runs = platform.runs.borrow_mut();
        let validation = runs.back_mut().unwrap();
        validation.outcome = MockPlatform::output(Some(0), stdout, stderr);
        drop(runs);
        assert!(matches!(
            validate(&platform, &layout),
            Err(Error::ProgramValidation { .. })
        ));
    }
}

#[test]
fn process_spawn_failures_are_preserved_at_both_boundaries() {
    for audit in [false, true] {
        let (platform, layout) = MockPlatform::nominal();
        reset_runs(&platform);
        if audit {
            platform.push_run(
                "/usr/bin/linuxcnc_var",
                &["LINUXCNCVERSION"],
                MockPlatform::output(Some(0), EXPECTED_LINUXCNC_VERSION, b""),
            );
            platform.push_run(
                layout.project.join("native/bin/dmc2-task-monitor"),
                &["--validate"],
                RunOutcome::Failure(io::ErrorKind::PermissionDenied),
            );
        } else {
            platform.push_run(
                "/usr/bin/linuxcnc_var",
                &["LINUXCNCVERSION"],
                RunOutcome::Failure(io::ErrorKind::NotFound),
            );
        }
        assert!(matches!(
            validate(&platform, &layout),
            Err(Error::OperatingSystem {
                operation: "execute process",
                ..
            })
        ));
    }
}
