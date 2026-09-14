//! Synchronous M190 persistence and data-planning gate for probing scripts.
//! No motion, command channel, reset, restart, or fault-clear capability.
mod block;
use dmc2ctl::probe_data::{ledger, schema};
mod storage;
mod surface;
mod tool_setter;

use schema::Workflow;
use std::env;
use std::ffi::{c_char, CString};
use std::path::PathBuf;

extern "C" {
    fn dmc2_probe_capture_error(nml_file: *const c_char, message: *const c_char) -> i32;
    fn dmc2_probe_capture_position(nml_file: *const c_char, xyz: *mut f64) -> i32;
}

fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--export-tool-offset") {
        // Historical export is data-only and never opens the machine channel.
        let result = match args.as_slice() {
            [_, path] => tool_setter::export(std::path::Path::new(path)),
            [_, path, ini, reference] => {
                let path = std::path::Path::new(path);
                tool_setter::snapshot(
                    path,
                    std::path::Path::new(ini),
                    std::path::Path::new(reference),
                    tool_setter::SnapshotTiming::AfterCapture,
                )
                .and_then(|_| tool_setter::export(path))
            }
            _ => Err("use --export-tool-offset <ledger> [<INI> <accepted-setter-reference.json>]; supply the reference files only for a historical ledger without its own snapshot".into()),
        };
        if let Err(error) = result {
            eprintln!("Tool offset export failed; retained contacts were preserved: {error}");
            std::process::exit(1);
        }
        return;
    }
    if matches!(
        args.first().map(String::as_str),
        Some("--export-surface" | "--export-block")
    ) {
        // Offline data export must never publish a live machine error.
        let result = if args.len() == 2 {
            if args[0] == "--export-block" {
                block::export(std::path::Path::new(&args[1]), false)
            } else {
                surface::export(std::path::Path::new(&args[1]), false)
            }
        } else {
            Err("use --export-surface or --export-block followed by a retained ledger path".into())
        };
        if let Err(error) = result {
            eprintln!("Probe data export failed; retained contacts were not changed: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run() {
        let message = if args.first().and_then(|s| integer(s).ok()) == Some(6) {
            format!("Gauge-block planning stopped: {error} Captures are retained. Use Abort, then Pendant Mode; correct the reported condition before a new Run.")
        } else {
            format!(
            "CAPTURE FAILED - KEEP THE SETUP IN PLACE. Abort, then Pendant Mode. Recording error: {error}"
        )
        };
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
        println!("dmc2-probe-capture is the synchronous M190 capture gate. Circle: P0 Q0 begins, P1 Q<sequence> saves. Surface: P2 Q0 begins, P3 Q<sequence> saves. Block: P4 Q0 begins, P5 Q<sequence> saves, P6 Q<sequence> publishes the next retained-data plan. Tool setter: P7 Q0 snapshots calibration and begins; P8 Q<sequence> saves and exports the fine contact's plate-referenced Z tool offset. Records are durably saved and read back. --export-surface <ledger>, --export-block <ledger>, and --export-tool-offset <ledger> [<INI> <accepted-setter-reference.json>] export retained data without a machine connection. No machine commands are issued.");
        return Ok(());
    }
    if args.len() != 2 {
        return Err(
            "M190 requires P<action> Q<sequence>; reopen the selected probing script".to_owned(),
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
        0 if sequence == 0 => storage::begin(&output, Workflow::Circle),
        1 => storage::commit(&output, Workflow::Circle, sequence, trigger_position),
        2 if sequence == 0 => storage::begin(&output, Workflow::Surface),
        3 => storage::commit(&output, Workflow::Surface, sequence, trigger_position),
        4 if sequence == 0 => {
            block::invalidate()?;
            storage::begin(&output, Workflow::Block)
        }
        5 => storage::commit(&output, Workflow::Block, sequence, trigger_position),
        6 => block::publish_next(&output, sequence),
        7 if sequence == 0 => {
            storage::begin(&output, Workflow::ToolSetter)?;
            let path = storage::active_path(&output, Workflow::ToolSetter, 0)?;
            tool_setter::snapshot(
                &path,
                &root.join("live/dmc2.ini"),
                &root.join("config/metrology/tool-setter.json"),
                tool_setter::SnapshotTiming::BeforeContact,
            )
        }
        8 => storage::commit(&output, Workflow::ToolSetter, sequence, trigger_position),
        _ => Err("unknown capture action; reopen the selected probing script".to_owned()),
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
