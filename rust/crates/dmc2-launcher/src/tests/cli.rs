use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

use crate::cli::{parse, Command, Mode};
use crate::error::Error;

fn arguments(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn every_valid_mode_has_one_exact_argument_form() {
    let cases = [
        (&[][..], Command::Launch(Mode::Validate)),
        (&["--help"][..], Command::Help),
        (&["-h"][..], Command::Help),
        (&["--live"][..], Command::Launch(Mode::Direct)),
        (
            &["--live", "--persistent"][..],
            Command::Launch(Mode::Persistent),
        ),
        (
            &["--persistent", "--live"][..],
            Command::Launch(Mode::Persistent),
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(parse(&arguments(source)), Ok(expected), "{source:?}");
    }
}

#[test]
fn duplicate_and_combined_help_forms_fail_closed() {
    for source in [
        &["--live", "--live"][..],
        &["--persistent", "--persistent"][..],
        &["--help", "--help"][..],
        &["-h", "-h"][..],
        &["--help", "-h"][..],
        &["--help", "--live"][..],
        &["--persistent", "-h"][..],
    ] {
        assert!(matches!(parse(&arguments(source)), Err(Error::Usage(_))));
    }
}

#[test]
fn persistent_without_live_and_every_unknown_token_fail_closed() {
    for source in [
        &["--persistent"][..],
        &["--validate"][..],
        &["live"][..],
        &[""][..],
        &["--LIVE"][..],
    ] {
        assert!(matches!(parse(&arguments(source)), Err(Error::Usage(_))));
    }
}

#[test]
fn every_non_utf8_argument_byte_is_rejected_without_loss() {
    for byte in 0x80_u8..=0xff {
        let result = parse(&[OsString::from_vec(vec![byte])]);
        let Err(Error::Usage(message)) = result else {
            panic!("byte {byte:#04x} was not rejected");
        };
        assert!(message.contains(&format!("\\x{byte:02x}")));
    }
}
