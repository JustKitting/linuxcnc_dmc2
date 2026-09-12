//! Synchronous M190 persistence gate for find-circle-center.ngc.
//! No motion, command channel, reset, restart, or fault-clear capability.
mod storage;

use std::env;
use std::ffi::{c_char, CString};
use std::path::PathBuf;

extern "C" {
    fn dmc2_probe_capture_error(nml_file: *const c_char, message: *const c_char) -> i32;
    fn dmc2_probe_capture_position(nml_file: *const c_char, xyz: *mut f64) -> i32;
}

fn main() {
    if let Err(error) = run() {
        let message = format!(
            "CAPTURE FAILED - KEEP THE SETUP IN PLACE. Abort, then Pendant Mode. Recording error: {error}"
        );
        eprintln!("{message}");
        let nml = env::var("EMC2_NMLFILE")
            .unwrap_or_else(|_| "/usr/share/linuxcnc/linuxcnc.nml".to_owned());
        let sent = match (CString::new(nml), CString::new(message)) {
            (Ok(nml), Ok(message)) => unsafe {
                dmc2_probe_capture_error(nml.as_ptr(), message.as_ptr()) == 0
            },
            _ => false,
        };
        if !sent {
            eprintln!("The LinuxCNC operator-error channel also rejected the capture diagnostic; the M190 failure stops the task. Use Abort and Pendant Mode in AXIS.");
        }
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args == ["--help"] {
        println!("dmc2-probe-capture is the synchronous M190 capture gate. P0 Q0 starts a new ledger; P1 Q<sequence> durably saves and reads back a staged record. No machine commands are issued.");
        return Ok(());
    }
    if args.len() != 2 {
        return Err(
            "M190 requires P<action> Q<sequence>; reopen find-circle-center.ngc".to_owned(),
        );
    }
    let action = integer(&args[0])?;
    let sequence = integer(&args[1])?;
    let binary = env::current_exe().map_err(|e| format!("locating the standard binary: {e}"))?;
    let root = binary
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .ok_or("capture binary is not installed under native/bin")?;
    if binary.parent() != Some(root.join("native/bin").as_path()) {
        return Err("use the installed native/bin/dmc2-probe-capture binary".to_owned());
    }
    let output: PathBuf = root.join("tmp/output");
    match action {
        0 if sequence == 0 => storage::begin(&output),
        1 => storage::commit(&output, sequence, trigger_position),
        _ => Err("unknown capture action; reopen find-circle-center.ngc".to_owned()),
    }
}

fn trigger_position() -> Result<[f64; 3], String> {
    let nml = CString::new(
        env::var("EMC2_NMLFILE").unwrap_or_else(|_| "/usr/share/linuxcnc/linuxcnc.nml".to_owned()),
    )
    .map_err(|e| format!("invalid NML path: {e}"))?;
    let mut xyz = [f64::NAN; 3];
    if unsafe { dmc2_probe_capture_position(nml.as_ptr(), xyz.as_mut_ptr()) } != 0 {
        return Err("LinuxCNC did not supply a valid original G38 trigger; no stopped-position fallback is used".into());
    }
    Ok(xyz)
}

fn integer(text: &str) -> Result<u64, String> {
    let value = text
        .parse::<f64>()
        .map_err(|e| format!("invalid M190 action/sequence {text:?}: {e}"))?;
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > u32::MAX as f64 {
        return Err(format!(
            "M190 action/sequence must be an unsigned integer: {text:?}"
        ));
    }
    Ok(value as u64)
}
