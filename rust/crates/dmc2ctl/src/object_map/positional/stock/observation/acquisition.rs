//! Common original-frame and companion checks for observation exports/history.
use crate::{
    object_map::{store::CaptureSnapshot, Error},
    probe_data::{ledger::number, mapper_schema::START_FIELDS, mapper_settings::close},
};

pub(super) fn snapshot(c: &CaptureSnapshot, name: &str) -> Result<String, Error> {
    let bytes = c.context.snapshots().find(|(k,_)|*k==name).and_then(|(_,b)|b).ok_or_else(||Error::Data(format!("The original {name} snapshot is missing. Preserve the source and import its intact companions before exporting.")))?;
    String::from_utf8(bytes.to_vec())
        .map_err(|e| Error::Data(format!("Original {name} snapshot is not UTF-8: {e}.")))
}
pub(super) fn compatible(primary: &CaptureSnapshot, other: &CaptureSnapshot) -> Result<(), Error> {
    // Ignore mode only: original top and explicit follow-up modes can share
    // these acquisition settings. Numerical equality is not physical alignment.
    for key in START_FIELDS.iter().filter(|&&k| k != "mode") {
        let a = number(&primary.capture.records[0], key).map_err(Error::Data)?;
        let b = number(&other.capture.records[0], key).map_err(Error::Data)?;
        if !close(a, b) {
            return Err(Error::Data(format!(
                "Capture {} starting field {key}={b} differs from source {} ({a}). Exclude this capture or prepare a separate analysis in its original acquisition frame.",
                other.id.as_str(), primary.id.as_str()
            )));
        }
    }
    for (kind, bytes) in primary
        .context
        .snapshots()
        .filter(|(k, _)| matches!(*k, "plate" | "feeds"))
    {
        let other_bytes = other
            .context
            .snapshots()
            .find(|(k, _)| *k == kind)
            .and_then(|(_, b)| b);
        if bytes != other_bytes {
            return Err(Error::Data(format!(
                "Capture {} has a different original {kind} snapshot from source {}. Exclude it or prepare an analysis with matching original settings; current configuration cannot substitute.",
                other.id.as_str(), primary.id.as_str()
            )));
        }
    }
    Ok(())
}
