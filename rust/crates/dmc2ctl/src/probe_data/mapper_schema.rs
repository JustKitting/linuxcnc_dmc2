//! Exact numeric contract shared by M190 validation and retained-data replay.
use std::collections::BTreeMap;
pub const START_FIELDS: &[&str] = &[
    "mode",
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
    "drop",
    "grid",
    "resolution",
    "usable_reach",
    "reach_reserve",
    "side_depth",
    "backoff",
    "step_x",
    "step_y",
    "step_z",
    "max_feed",
];
pub const EVENT_FIELDS: &[&str] = &[
    "sample",
    "phase",
    "edge",
    "stage",
    "success",
    "approach_x",
    "approach_y",
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
pub(super) fn validate(kind: &str, fields: &BTreeMap<&str, &str>) -> Result<(), String> {
    let n = |k| fields[k].parse::<f64>().unwrap(); // finite in the common schema
    if kind == "start" {
        return Ok(());
    }
    for key in ["sample", "phase", "edge", "stage", "success"] {
        if n(key).fract() != 0.0 || n(key) < -1.0 || n(key) > i32::MAX as f64 {
            return Err(format!("Mapper {key} is not a valid integer identifier."));
        }
    }
    if n("phase") > 5.0
        || n("phase") < 0.0
        || n("edge") > 3.0
        || n("sample") < 0.0
        || n("stage") > 1.0
        || n("feed") <= 0.0
    {
        return Err("Mapper phase, face, sample, touch stage or feed is invalid.".into());
    }
    if !matches!(n("success"), 0.0 | 1.0)
        || super::schema::Workflow::Mapper.requires_exact_trigger(kind) != (n("success") == 1.0)
    {
        return Err("Mapper contact classification disagrees with the original G38 result.".into());
    }
    if matches!(kind, "recontact" | "withdrawal-release")
        && (n("stage") != -1.0
            || n("target_x") != n("from_x")
            || n("target_y") != n("from_y")
            || n("target_z") <= n("from_z"))
    {
        return Err("A withdrawal event must describe upward Z-only travel with unchanged XY and no measurement stage.".into());
    }
    if kind == "miss" && n("stage") != 0.0 {
        return Err("A failed fine touch cannot be accepted as air.".into());
    }
    Ok(())
}
