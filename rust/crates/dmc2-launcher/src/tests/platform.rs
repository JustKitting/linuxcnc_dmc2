use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::platform::{CommandSpec, Platform, RealPlatform};

#[test]
fn real_filesystem_adapter_distinguishes_files_directories_and_missing_paths() {
    let platform = RealPlatform;
    let executable = platform.current_executable().unwrap();
    assert!(platform.is_regular_file(&executable).unwrap());
    assert!(!platform
        .read_file(&executable)
        .expect("test executable must be readable")
        .is_empty());
    assert!(!platform
        .is_regular_file(executable.parent().unwrap())
        .unwrap());
    assert!(platform
        .is_regular_file(&PathBuf::from("/definitely/missing/dmc2-launcher"))
        .is_err());
}

#[test]
fn real_executable_search_handles_explicit_path_path_lookup_and_rejections() {
    let platform = RealPlatform;
    let executable = platform.current_executable().unwrap();
    assert_eq!(
        platform
            .find_executable(executable.to_str().unwrap())
            .unwrap(),
        Some(executable)
    );
    assert!(platform.find_executable("sh").unwrap().is_some());
    assert_eq!(platform.find_executable("/etc/passwd").unwrap(), None);
    assert_eq!(
        platform
            .find_executable("/definitely/missing/dmc2")
            .unwrap(),
        None
    );
    assert_eq!(
        platform.find_executable("dmc2-no-such-executable").unwrap(),
        None
    );
    assert_eq!(
        platform
            .find_executable("/invalid/\0path")
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn real_process_adapter_preserves_status_stdout_stderr_environment_and_directory() {
    let platform = RealPlatform;
    let mut command = CommandSpec::new("/bin/sh");
    command.arguments = vec![
        OsString::from("-c"),
        OsString::from(
            "printf '%s:%s' \"$DMC2_ADAPTER_VALUE\" \"$PWD\"; printf 'err-byte' >&2; exit 7",
        ),
    ];
    command.environment.insert(
        OsString::from("DMC2_ADAPTER_VALUE"),
        OsString::from("exact-value"),
    );
    command.working_directory = Some(PathBuf::from("/tmp"));
    let output = platform.run(&command).unwrap();
    assert_eq!(output.status, Some(7));
    assert_eq!(output.stdout, b"exact-value:/tmp");
    assert_eq!(output.stderr, b"err-byte");

    let command = CommandSpec::new("/definitely/missing/dmc2-command");
    assert!(platform.run(&command).is_err());
}

#[test]
fn real_process_adapter_reports_signal_termination_without_inventing_an_exit_code() {
    let platform = RealPlatform;
    let mut command = CommandSpec::new("/bin/sh");
    command.arguments = vec![OsString::from("-c"), OsString::from("kill -TERM $$")];
    let output = platform.run(&command).unwrap();
    assert_eq!(output.status, None);
}

#[test]
fn real_path_absence_is_tested_in_an_isolated_child_environment() {
    if std::env::var_os("DMC2_TEST_PATH_ABSENT").is_some() {
        assert_eq!(RealPlatform.find_executable("sh").unwrap(), None);
        return;
    }
    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("tests::platform::real_path_absence_is_tested_in_an_isolated_child_environment")
        .arg("--nocapture")
        .env("DMC2_TEST_PATH_ABSENT", "1")
        .env_remove("PATH")
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn real_path_lookup_preserves_candidate_filesystem_errors() {
    if std::env::var_os("DMC2_TEST_PATH_ERROR").is_some() {
        assert_eq!(
            RealPlatform
                .find_executable("blocked-executable")
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        return;
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "dmc2-launcher-inaccessible-path-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o000)).unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("tests::platform::real_path_lookup_preserves_candidate_filesystem_errors")
        .arg("--nocapture")
        .env("DMC2_TEST_PATH_ERROR", "1")
        .env("PATH", &directory)
        .status()
        .unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_dir(&directory).unwrap();
    assert!(status.success());
}

#[test]
fn real_process_replacement_is_exercised_in_an_isolated_child() {
    if std::env::var_os("DMC2_TEST_EXEC_REPLACE").is_some() {
        let result = RealPlatform.replace_process(&CommandSpec::new("/bin/true"));
        panic!("successful process replacement returned: {result:?}");
    }
    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("tests::platform::real_process_replacement_is_exercised_in_an_isolated_child")
        .arg("--nocapture")
        .env("DMC2_TEST_EXEC_REPLACE", "1")
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn real_process_replacement_preserves_exec_failure() {
    let result = RealPlatform.replace_process(&CommandSpec::new(
        "/definitely/missing/dmc2-process-replacement",
    ));
    assert!(result.is_err());
}
