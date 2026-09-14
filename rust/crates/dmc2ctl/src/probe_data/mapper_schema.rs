//! Exact numeric contract shared by M190 validation and retained-data replay.
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    Reference = 0,
    Boundary = 1,
    Grid = 2,
    Verify = 3,
    Rim = 4,
    Finished = 5,
    OutlineEnter = 6,
    OutlineAdvance = 7,
    OutlineBackoff = 8,
    OutlineClose = 9,
}
impl Phase {
    pub fn read(value: f64) -> Result<Self, String> {
        match value {
            0.0 => Ok(Self::Reference),
            1.0 => Ok(Self::Boundary),
            2.0 => Ok(Self::Grid),
            3.0 => Ok(Self::Verify),
            4.0 => Ok(Self::Rim),
            5.0 => Ok(Self::Finished),
            6.0 => Ok(Self::OutlineEnter),
            7.0 => Ok(Self::OutlineAdvance),
            8.0 => Ok(Self::OutlineBackoff),
            9.0 => Ok(Self::OutlineClose),
            _ => Err("Unknown mapper phase; reopen the matching script before Run.".into()),
        }
    }
    pub fn is_outline(self) -> bool {
        matches!(
            self,
            Self::OutlineEnter | Self::OutlineAdvance | Self::OutlineBackoff | Self::OutlineClose
        )
    }
}
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
    if Phase::read(n("phase")).is_err()
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
    if matches!(
        kind,
        "recontact" | "withdrawal-release" | "withdrawal-complete"
    ) {
        let complete = kind == "withdrawal-complete";
        let upward = n("target_x") == n("from_x")
            && n("target_y") == n("from_y")
            && (n("target_z") > n("from_z") || (complete && n("target_z") == n("from_z")));
        let lateral = Phase::read(n("phase"))?.is_outline()
            && n("target_z") == n("from_z")
            && (complete || n("target_x") != n("from_x") || n("target_y") != n("from_y"));
        if n("stage") != -1.0 || !(upward || lateral) {
            return Err("Withdrawal must retain an upward Z path or fixed-Z outline return, with no measurement stage.".into());
        }
    }
    if kind == "miss" && n("stage") != 0.0 {
        return Err("A failed fine touch cannot be accepted as air.".into());
    }
    Ok(())
}
