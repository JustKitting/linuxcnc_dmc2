//! Adaptive cuboid metrology: Rust plans, the typed G-code script executes.
mod exchange;
mod geometry;
mod model;
mod report;
#[cfg(test)]
mod tests;

use super::{ledger, schema::Workflow, storage};
use std::collections::BTreeMap;
use std::path::Path;

pub(super) use exchange::invalidate;
pub(super) use report::export;

pub(super) const START_FIELDS: &[&str] = &[
    "x",
    "y",
    "z",
    "offset_x",
    "offset_y",
    "offset_z",
    "x_min",
    "x_max",
    "y_min",
    "y_max",
    "z_min",
    "z_max",
    "search",
    "grid",
    "drop",
    "clearance",
    "side_depth",
    "feed",
    "coarse_feed",
    "ball_diameter",
    "step_x",
    "step_y",
    "step_z",
];
pub(super) const EVENT_FIELDS: &[&str] = &[
    "sample",
    "phase",
    "column",
    "row",
    "edge",
    "layer",
    "stage",
    "success",
    "work_x",
    "work_y",
    "work_z",
    "machine_x",
    "machine_y",
    "machine_z",
    "from_x",
    "from_y",
    "from_z",
    "target_x",
    "target_y",
    "target_z",
    "feed",
];

pub(super) fn validate_fields(kind: &str, fields: &BTreeMap<&str, &str>) -> Result<(), String> {
    let n = |key: &str| fields[key].parse::<f64>().unwrap(); // already finite in shared schema
    if kind == "start" {
        for key in [
            "search",
            "grid",
            "drop",
            "clearance",
            "side_depth",
            "feed",
            "coarse_feed",
            "ball_diameter",
            "step_x",
            "step_y",
            "step_z",
        ] {
            if n(key) <= 0.0 {
                return Err(format!("Block setting {key} must be positive."));
            }
        }
        // These are this operation's user-requested geometry, not hidden knobs.
        if n("grid") != 1.0
            || n("ball_diameter") != 2.0
            || n("feed") != 50.0
            || n("coarse_feed") != 200.0
        {
            return Err("Block script settings do not match its 1 mm grid, nominal 2 mm ball and 50/200 mm/min touch feeds.".into());
        }
        for axis in ["x", "y", "z"] {
            if n(&format!("{axis}_min")) >= n(&format!("{axis}_max")) {
                return Err(format!("Invalid {axis} travel limits."));
            }
        }
    } else {
        for key in [
            "sample", "phase", "column", "row", "edge", "layer", "stage", "success",
        ] {
            let value = n(key);
            if value.fract() != 0.0 || value < i32::MIN as f64 || value > i32::MAX as f64 {
                return Err(format!("Block {key} must be an exact signed integer."));
            }
        }
        if !(0.0..=2.0).contains(&n("phase"))
            || !(0.0..=2.0).contains(&n("layer"))
            || n("sample") < 0.0
            || n("feed") <= 0.0
        {
            return Err(
                "Block phase, circuit, sample or feed is outside the operation's definition."
                    .into(),
            );
        }
        if !matches!(n("success"), 0.0 | 1.0)
            || matches!(kind, "touch" | "obstruction") != (n("success") == 1.0)
        {
            return Err(
                "Block contact/miss classification disagrees with the original probe result."
                    .into(),
            );
        }
        if kind == "miss" && n("stage") != 0.0 {
            return Err("A failed slow re-touch is not an accepted grid miss.".into());
        }
    }
    Ok(())
}

pub(super) fn publish_next(output: &Path, sequence: u64) -> Result<(), String> {
    exchange::with_bank(|bank| {
        bank.invalidate()?;
        let path = storage::active_path(output, Workflow::Block, sequence)?;
        let records = ledger::records(
            &std::fs::read_to_string(&path).map_err(|e| e.to_string())?,
            Workflow::Block,
        )?;
        if records.len() as u64 != sequence {
            return Err("Block ledger length disagrees with the requested plan sequence.".into());
        }
        let state = model::State::read(&records)?;
        let plan = state.next()?;
        let values = plan.values(sequence);
        let saved = values
            .iter()
            .map(|(k, v)| format!("{k}={v}\n"))
            .collect::<String>();
        ledger::publish(
            &path.with_extension(format!("plan-{sequence}.txt")),
            saved.as_bytes(),
        )?;
        bank.publish(&values)
    })
}
