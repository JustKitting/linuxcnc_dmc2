//! Save the calibration and export tool Z compensation without applying it.
mod model;
#[cfg(test)]
mod tests;

use crate::ledger::publish;
use model::{Calibration, ToolOffset};
use std::fs;
use std::path::Path;

#[derive(Clone, Copy)]
pub(super) enum SnapshotTiming {
    BeforeContact,
    AfterCapture,
}

impl SnapshotTiming {
    fn name(self) -> &'static str {
        match self {
            Self::BeforeContact => "at-measurement-start",
            Self::AfterCapture => "explicitly-supplied-for-retained-measurement",
        }
    }
}

pub(super) fn snapshot(
    path: &Path,
    ini: &Path,
    reference: &Path,
    timing: SnapshotTiming,
) -> Result<(), String> {
    let ini = fs::read_to_string(ini).map_err(|e| format!("reading tool-setting INI: {e}"))?;
    let reference = fs::read_to_string(reference)
        .map_err(|e| format!("reading accepted setter reference: {e}"))?;
    Calibration::read(&ini, &reference)?;
    publish(&path.with_extension("tool-reference.ini"), ini.as_bytes())?;
    publish(
        &path.with_extension("setter-reference.json"),
        reference.as_bytes(),
    )?;
    publish(
        &path.with_extension("tool-reference-timing.txt"),
        timing.name().as_bytes(),
    )?;
    Ok(())
}

pub(super) fn export(path: &Path) -> Result<(), String> {
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .ok_or("tool ledger filename must be its original plain measurement identifier")?;
    let text =
        fs::read_to_string(path).map_err(|e| format!("reading retained tool contacts: {e}"))?;
    let ini = fs::read_to_string(path.with_extension("tool-reference.ini"))
        .map_err(|e| format!("reading saved calibration INI: {e}; historical exports need the explicit reference paths"))?;
    let reference = fs::read_to_string(path.with_extension("setter-reference.json"))
        .map_err(|e| format!("reading saved setter calibration: {e}"))?;
    let timing = fs::read_to_string(path.with_extension("tool-reference-timing.txt"))
        .map_err(|e| format!("reading calibration provenance: {e}"))?;
    if ![
        SnapshotTiming::BeforeContact.name(),
        SnapshotTiming::AfterCapture.name(),
    ]
    .contains(&timing.as_str())
    {
        return Err("saved calibration snapshot timing is unknown".into());
    }
    let calibration = Calibration::read(&ini, &reference)?;
    let measurement = ToolOffset::from_ledger(&text, calibration)?;
    let result = path.with_extension("tool-offset.json");
    let program = path.with_extension("tool-offset.ngc");
    publish(
        &result,
        measurement.json(id, calibration, &timing).as_bytes(),
    )?;
    publish(&program, measurement.apply_program(id).as_bytes())?;
    println!("DMC2 tool offset saved and read back: {}\nplate_referenced_tool_offset_z_mm={}\napply_offset_file={}\noffset_applied=false", result.display(), measurement.offset_z_mm, program.display());
    Ok(())
}
