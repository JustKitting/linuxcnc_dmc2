//! Explicit top columns using original acquisition settings and fresh cycles.
//! Data only: the shared LinuxCNC executor owns all movement and withdrawal.
mod format;
mod program;
mod role;
pub use role::Role;
mod rows;
pub use rows::{Repeat, Rows};
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
    pub rows: Rows,
    /// None retains the historical V1 plan bytes and implicit fitting role.
    pub role: Option<Role>,
}
impl Plan {
    pub fn settings(&self) -> Result<Settings, String> {
        let s = Settings::read(
            &self.start,
            &data(&self.plate, "DMC2_PLATE_ENVELOPE_V1", PLATE_FIELDS)?,
            &policy(&self.feeds)?,
            None,
        )?;
        if self.rows.repeated() && self.role != Some(Role::Check) {
            return Err("Original top repeats must retain contact_role=check. Re-export their observation analysis; original contacts and failed checks cannot be replaced by new fitting rows.".into());
        }
        if self.rows.directed() && self.role.is_none() {
            return Err("Adaptive measurements require an explicit fit/check contact role. Regenerate the plan with its intended evidence role.".into());
        }
        self.rows.requests(&s)?;
        Ok(s)
    }

    pub fn requests(&self) -> Result<Vec<Request>, String> {
        let s = self.settings()?;
        self.rows.requests(&s)
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
            let mut expected = vec![
                ("phase", q.phase as u8 as f64),
                ("edge", q.edge as f64),
                ("approach_x", q.approach[0]),
                ("approach_y", q.approach[1]),
                ("target_x", q.target[0]),
                ("target_y", q.target[1]),
                ("target_z", q.target[2]),
                (
                    "feed",
                    if stage == 0.0 {
                        s.coarse_feed(q.phase)
                    } else {
                        s.feeds[1]
                    },
                ),
            ];
            if !self.rows.directed() || stage == 0.0 {
                expected.extend([("from_x", q.approach[0]), ("from_y", q.approach[1])]);
            }
            for (key, value) in expected {
                if !close(number(r, key)?, value) {
                    return Err(format!("Follow-up record {} disagrees with its planned {key}. Preserve the capture and plan; Abort then Pendant Mode. This record cannot supply material evidence.", r["sequence"]));
                }
            }
            let from_z = if q.phase == crate::probe_data::mapper_schema::Phase::Rim {
                q.target[2]
            } else {
                s.origin[2]
            };
            if stage == 0.0 && !close(number(r, "from_z")?, from_z) {
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
