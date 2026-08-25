use std::collections::BTreeSet;
use std::io;

use crate::embedded;
use crate::error::Error;
use crate::integrity::{deployments, validate_deployments, validate_embedded_inputs};

use super::support::{FileEntry, MockPlatform};

#[test]
fn embedded_launch_surface_is_unique_and_covers_both_projects() {
    let identities: BTreeSet<_> = embedded::FILES
        .iter()
        .map(|file| (file.root, file.relative))
        .collect();
    assert_eq!(identities.len(), embedded::FILES.len());
    assert!(embedded::FILES
        .iter()
        .any(|file| file.root == embedded::Root::Project));
    assert!(embedded::FILES
        .iter()
        .any(|file| file.root == embedded::Root::H100));
}

#[test]
fn every_embedded_input_is_checked_for_missing_type_read_and_content_failures() {
    for file in embedded::FILES {
        let (platform, layout) = MockPlatform::nominal();
        let root = match file.root {
            embedded::Root::Project => &layout.project,
            embedded::Root::H100 => &layout.h100,
        };
        let path = root.join(file.relative);
        platform.files.borrow_mut().remove(&path);
        assert_eq!(
            validate_embedded_inputs(&platform, &layout),
            Err(Error::NotRegularFile(path.clone()))
        );

        let (platform, layout) = MockPlatform::nominal();
        platform
            .files
            .borrow_mut()
            .insert(path.clone(), FileEntry::Other);
        assert_eq!(
            validate_embedded_inputs(&platform, &layout),
            Err(Error::NotRegularFile(path.clone()))
        );

        let (platform, layout) = MockPlatform::nominal();
        platform.files.borrow_mut().insert(
            path.clone(),
            FileEntry::Failure(io::ErrorKind::PermissionDenied),
        );
        assert!(matches!(
            validate_embedded_inputs(&platform, &layout),
            Err(Error::OperatingSystem {
                operation: "inspect regular file",
                ..
            })
        ));

        let (platform, layout) = MockPlatform::nominal();
        platform.files.borrow_mut().insert(
            path.clone(),
            FileEntry::ReadFailure(io::ErrorKind::InvalidData),
        );
        assert!(matches!(
            validate_embedded_inputs(&platform, &layout),
            Err(Error::OperatingSystem {
                operation: "read file",
                ..
            })
        ));

        let (platform, layout) = MockPlatform::nominal();
        platform
            .files
            .borrow_mut()
            .insert(path.clone(), FileEntry::Regular(b"changed".to_vec()));
        assert_eq!(
            validate_embedded_inputs(&platform, &layout),
            Err(Error::EmbeddedFileChanged(path))
        );
    }
}

#[test]
fn all_five_deployments_are_exactly_byte_compared() {
    let (platform, layout) = MockPlatform::nominal();
    assert_eq!(deployments(&layout).len(), 5);
    validate_deployments(&platform, &layout).unwrap();

    for (deployed, staged) in deployments(&layout) {
        let (platform, layout) = MockPlatform::nominal();
        platform
            .files
            .borrow_mut()
            .insert(deployed.clone(), FileEntry::Regular(b"changed".to_vec()));
        assert_eq!(
            validate_deployments(&platform, &layout),
            Err(Error::DeploymentMismatch {
                deployed: deployed.clone(),
                staged: staged.clone(),
            })
        );

        for missing in [&deployed, &staged] {
            let (platform, layout) = MockPlatform::nominal();
            platform.files.borrow_mut().remove(missing);
            assert_eq!(
                validate_deployments(&platform, &layout),
                Err(Error::NotRegularFile(missing.clone()))
            );
        }

        for unreadable in [&deployed, &staged] {
            let (platform, layout) = MockPlatform::nominal();
            platform.files.borrow_mut().insert(
                unreadable.clone(),
                FileEntry::ReadFailure(io::ErrorKind::PermissionDenied),
            );
            assert!(matches!(
                validate_deployments(&platform, &layout),
                Err(Error::OperatingSystem {
                    operation: "read file",
                    ..
                })
            ));

            let (platform, layout) = MockPlatform::nominal();
            platform
                .files
                .borrow_mut()
                .insert(unreadable.clone(), FileEntry::Other);
            assert_eq!(
                validate_deployments(&platform, &layout),
                Err(Error::NotRegularFile(unreadable.clone()))
            );
        }
    }
}
