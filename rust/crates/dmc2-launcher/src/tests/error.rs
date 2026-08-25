use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use crate::error::{render_bytes, Error, OwnerMatch};

#[test]
fn byte_renderer_accounts_for_every_possible_byte() {
    let bytes: Vec<u8> = (0_u8..=u8::MAX).collect();
    let rendered = render_bytes(&bytes);
    for byte in 0_u8..=u8::MAX {
        let expected = match byte {
            b' '..=b'~' if byte != b'\\' => char::from(byte).to_string(),
            b'\\' => "\\\\".to_owned(),
            b'\n' => "\\n".to_owned(),
            b'\r' => "\\r".to_owned(),
            b'\t' => "\\t".to_owned(),
            other => format!("\\x{other:02x}"),
        };
        assert!(rendered.contains(&expected), "missing byte {byte:#04x}");
    }
}

#[test]
fn every_error_variant_has_operator_visible_context() {
    let variants = [
        Error::Usage("bad argument".to_owned()),
        Error::OperatingSystem {
            operation: "read",
            target: PathBuf::from("/target"),
            code: Some(5),
            detail: "io detail".to_owned(),
        },
        Error::ProjectRootNotFound(PathBuf::from("/start")),
        Error::NotRegularFile(PathBuf::from("/file")),
        Error::EmbeddedFileChanged(PathBuf::from("/changed")),
        Error::DeploymentMismatch {
            deployed: PathBuf::from("/deployed"),
            staged: PathBuf::from("/staged"),
        },
        Error::ExecutableUnavailable("program"),
        Error::ProcessFailed {
            program: PathBuf::from("program"),
            status: None,
            stdout: vec![0xff],
            stderr: vec![0],
        },
        Error::LinuxCncVersion {
            stdout: b"2.9.9".to_vec(),
            stderr: b"version warning".to_vec(),
        },
        Error::ProgramValidation {
            stdout: b"validation".to_vec(),
            stderr: b"validation warning".to_vec(),
        },
        Error::OwnerConflict(vec![OwnerMatch {
            pattern: "owner",
            stdout: b"123".to_vec(),
            stderr: b"warning".to_vec(),
        }]),
        Error::OwnerProbe {
            pattern: "owner",
            status: Some(2),
            stdout: vec![1],
            stderr: vec![2],
        },
        Error::ExecReturned,
    ];
    for error in variants {
        assert!(!error.to_string().is_empty(), "{error:?}");
    }
}

#[test]
fn non_utf8_path_bytes_are_rendered_without_loss() {
    let path = PathBuf::from(OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff]));
    assert!(Error::NotRegularFile(path)
        .to_string()
        .ends_with("/tmp/\\xff"));
}
