use std::io;
use std::path::PathBuf;

use crate::error::Error;
use crate::layout::{discover, discover_from};

use super::support::{FileEntry, MockPlatform};

#[test]
fn root_is_discovered_from_both_deployed_and_staged_depths() {
    let (mut platform, expected) = MockPlatform::nominal();
    assert_eq!(discover(&platform), Ok(expected.clone()));
    platform.executable = expected.project.join("rust/target/release/dmc2-linuxcnc");
    assert_eq!(discover(&platform), Ok(expected));
}

#[test]
fn current_executable_is_never_probed_as_if_it_were_a_directory() {
    let (platform, expected) = MockPlatform::nominal();
    for marker in ["live/dmc2.ini", "live_requirements.json", "rust/Cargo.toml"] {
        platform.files.borrow_mut().insert(
            platform.executable.join(marker),
            FileEntry::Failure(io::ErrorKind::NotADirectory),
        );
    }
    assert_eq!(discover(&platform), Ok(expected));
}

#[test]
fn every_root_marker_is_mandatory_and_regular() {
    let markers = ["live/dmc2.ini", "live_requirements.json", "rust/Cargo.toml"];
    for marker in markers {
        let (platform, _) = MockPlatform::nominal();
        platform
            .files
            .borrow_mut()
            .remove(&PathBuf::from("/project").join(marker));
        assert!(matches!(
            discover_from(&platform, PathBuf::from("/project/native/bin").as_path()),
            Err(Error::ProjectRootNotFound(_))
        ));

        let (platform, _) = MockPlatform::nominal();
        platform
            .files
            .borrow_mut()
            .insert(PathBuf::from("/project").join(marker), FileEntry::Other);
        assert!(matches!(
            discover_from(&platform, PathBuf::from("/project/native/bin").as_path()),
            Err(Error::ProjectRootNotFound(_))
        ));
    }
}

#[test]
fn root_marker_io_failure_and_current_executable_failure_are_preserved() {
    let (platform, _) = MockPlatform::nominal();
    platform.files.borrow_mut().insert(
        PathBuf::from("/project/live/dmc2.ini"),
        FileEntry::Failure(io::ErrorKind::PermissionDenied),
    );
    assert!(matches!(
        discover(&platform),
        Err(Error::OperatingSystem {
            operation: "inspect project marker",
            ..
        })
    ));

    let (mut platform, _) = MockPlatform::nominal();
    platform.executable_failure = Some(io::ErrorKind::NotFound);
    assert!(matches!(
        discover(&platform),
        Err(Error::OperatingSystem {
            operation: "resolve current executable",
            ..
        })
    ));
}

#[test]
fn marker_set_at_filesystem_root_is_rejected_without_parent_invention() {
    let (platform, _) = MockPlatform::nominal();
    for marker in ["live/dmc2.ini", "live_requirements.json", "rust/Cargo.toml"] {
        platform.files.borrow_mut().insert(
            PathBuf::from("/").join(marker),
            FileEntry::Regular(b"root marker".to_vec()),
        );
    }
    assert_eq!(
        discover_from(&platform, PathBuf::from("/").as_path()),
        Err(Error::ProjectRootNotFound(PathBuf::from("/")))
    );

    let (mut platform, _) = MockPlatform::nominal();
    platform.executable = PathBuf::from("/");
    assert_eq!(
        discover(&platform),
        Err(Error::ProjectRootNotFound(PathBuf::from("/")))
    );
}
