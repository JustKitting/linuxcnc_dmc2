//! Explicit top columns using original acquisition settings and fresh cycles.
//! Data only: the shared LinuxCNC executor owns all movement and withdrawal.
mod format;
mod program;
#[cfg(test)]
mod tests;
use super::{
    ledger::{number, Fields},
    mapper_schema::START_FIELDS,
    mapper_settings::{close, data, policy, Mode, Request, Sample, Settings, PLATE_FIELDS},
    mapper_trace::{observation::TopColumn, state},
};

pub struct Plan {
    /// Original object, setup, observation analysis and source capture IDs.
    pub source: [String; 4],
    pub start: Fields,
    pub plate: String,
    pub feeds: String,
    /// Explicit execution order; each leg returns to original clearance.
    pub points: Vec<[f64; 2]>,
}
impl Plan {
    pub fn settings(&self) -> Result<Settings, String> {
        let s = Settings::read(
            &self.start,
            &data(&self.plate, "DMC2_PLATE_ENVELOPE_V1", PLATE_FIELDS)?,
            &policy(&self.feeds)?,
            None,
        )?;
        if self.points.is_empty() {
            return Err("The follow-up plan contains no selected top columns. Select an analysis with new points; no empty program was supplied.".into());
        }
        for &xy in &self.points {
            TopColumn::new(&s, xy)?;
        }
        Ok(s)
    }

    pub fn requests(&self) -> Result<Vec<Request>, String> {
        let s = self.settings()?;
        self.points
            .iter()
            .map(|&xy| TopColumn::new(&s, xy).map(|c| c.request))
            .collect()
    }

    /// Validate both runtime next-target state and imported measurement context.
    /// A finished/partial report cannot accept a different plan, frame or row.
    pub fn samples(&self, records: &[Fields], require_result: bool) -> Result<Vec<Sample>, String> {
        let s = self.settings()?;
        let actual = records.first().ok_or("The follow-up start record is missing; preserve the capture and use Abort then Pendant Mode.")?;
        if Mode::read(number(actual, "mode")?)? != Mode::TopFollowup {
            return Err("A top follow-up plan accompanies a different mapper mode. Preserve both originals and select the matching capture/program; no plan was substituted.".into());
        }
        for &key in START_FIELDS.iter().filter(|&&k| k != "mode") {
            if !close(number(actual, key)?, number(&self.start, key)?) {
                return Err(format!("Follow-up starting {key} differs from its original acquisition reference. No next target was supplied. Use Abort then Pendant Mode; establish the reviewed source frame/start or prepare a new acquisition."));
            }
        }
        let mut active = s.clone();
        active.mode = Mode::TopFollowup;
        let samples = state::samples(records, &active, require_result)?;
        let requests = self.requests()?;
        // Check original coarse AND fine records, including miss feeds and
        // their actual downward entry. Retained endpoints never become hits.
        for r in records
            .iter()
            .skip(1)
            .filter(|r| matches!(r["kind"].as_str(), "touch" | "miss"))
        {
            let index = number(r, "sample")?;
            let q = if index.fract() == 0.0 && index >= 0.0 {
                requests.get(index as usize)
            } else { None }.ok_or("A follow-up contact has no matching requested row. Preserve the capture; Abort then Pendant Mode before a new Run.")?;
            let stage = number(r, "stage")?;
            for (key, value) in [
                ("phase", q.phase as u8 as f64),
                ("edge", q.edge as f64),
                ("approach_x", q.approach[0]),
                ("approach_y", q.approach[1]),
                ("target_x", q.target[0]),
                ("target_y", q.target[1]),
                ("target_z", q.target[2]),
                ("from_x", q.approach[0]),
                ("from_y", q.approach[1]),
                (
                    "feed",
                    if stage == 0.0 {
                        s.downward_feed
                    } else {
                        s.feeds[1]
                    },
                ),
            ] {
                if !close(number(r, key)?, value) {
                    return Err(format!("Follow-up record {} disagrees with its planned {key}. Preserve the capture and plan; Abort then Pendant Mode. This record cannot supply material evidence.", r["sequence"]));
                }
            }
            if stage == 0.0 && !close(number(r, "from_z")?, s.origin[2]) {
                return Err("A follow-up coarse dip did not start at its retained clearance. Preserve the capture and recover through Abort then Pendant Mode.".into());
            }
        }
        let ended = records.last().is_some_and(|r| r["kind"] == "result");
        if samples.len() > requests.len() || (ended && samples.len() != requests.len()) {
            return Err("The follow-up result does not match the complete requested row count. Preserve the ledger and plan; start a new reviewed Run after UI recovery.".into());
        }
        Ok(samples)
    }
}
