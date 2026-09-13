//! Import original G38 ledgers; endpoint fields never become contacts.
use super::{
    model::{CaptureState, Contact, Stage},
    record::quote,
    Error,
};
use crate::probe_data::{
    ledger::{self, number, Fields},
    schema::Workflow,
};

pub struct Capture {
    pub workflow: Workflow,
    pub state: CaptureState,
    pub records: Vec<Fields>,
    pub contacts: Vec<Contact>,
    pub misses: usize,
    pub nominal_ball_diameter_mm: f64,
}

impl Capture {
    pub fn read(text: &str) -> Result<Self, Error> {
        Self::parse(text).map_err(Error::Data)
    }

    fn parse(text: &str) -> Result<Self, String> {
        let workflow = [Workflow::Circle, Workflow::Surface, Workflow::Block]
            .into_iter()
            .find(|w| text.lines().next() == Some(w.ledger_magic()))
            .ok_or("Only retained circle, surface and gauge-block G38 ledgers are supported.")?;
        if !text.ends_with('\n') || !text.lines().any(|l| l == "units=mm,mm/min") {
            return Err("Capture units or its final record terminator are missing.".into());
        }
        // The shared reader validates sequence, schema, finite values and exact bits.
        let records = ledger::records(text, workflow)?;
        let start = records
            .first()
            .filter(|r| r["kind"] == "start")
            .ok_or("Capture has no initial setup record.")?;
        let nominal_ball_diameter_mm = number(start, "ball_diameter")?;
        if nominal_ball_diameter_mm <= 0.0 {
            return Err("Capture ball diameter must be positive.".into());
        }
        // Reject ignored trailing/outside-record text; do not salvage a torn file.
        let mut in_record = false;
        let mut started = false;
        for line in text.lines() {
            if line.starts_with("BEGIN ") {
                if in_record {
                    return Err("Capture contains a nested record.".into());
                }
                in_record = true;
                started = true;
            } else if line.starts_with("END ") {
                if !in_record {
                    return Err("Capture contains an unmatched record ending.".into());
                }
                in_record = false;
            } else if started && !in_record {
                return Err("Capture has text outside its retained record boundaries.".into());
            }
        }
        let mut contacts = Vec::new();
        let mut misses = 0;
        let mut quarantined = false;
        let mut result = false;
        for (sequence, r) in records.iter().enumerate().skip(1) {
            if result || r["kind"] == "start" {
                return Err("Capture has records after its result or a repeated setup.".into());
            }
            match r["kind"].as_str() {
                "result" => result = true,
                "obstruction" => {
                    if r.get("exact_source").map(String::as_str)
                        != Some("emcStatus.motion.traj.probedPosition;machine-mm")
                    {
                        return Err(
                            "Obstruction is missing the original machine G38 source label.".into(),
                        );
                    }
                    quarantined = true;
                }
                "miss" => {
                    misses += 1;
                    if workflow == Workflow::Block && number(r, "phase")? == 2.0 {
                        quarantined = true;
                    }
                }
                "touch" => {
                    if r.get("exact_source").map(String::as_str)
                        != Some("emcStatus.motion.traj.probedPosition;machine-mm")
                    {
                        return Err(
                            "Contact is missing the original machine G38 source label.".into()
                        );
                    }
                    let stage = match r["stage"].as_str() {
                        "0" => Stage::Coarse,
                        "1" => Stage::Fine,
                        _ => unreachable!("schema validated stage"),
                    };
                    let mut trigger_mm = [0.0; 3];
                    for (i, axis) in ["x", "y", "z"].iter().enumerate() {
                        trigger_mm[i] = number(r, &format!("machine_{axis}_exact"))?;
                    }
                    let direction = if workflow == Workflow::Block {
                        let mut vector = [0.0; 3];
                        for (i, axis) in ["x", "y", "z"].iter().enumerate() {
                            vector[i] = number(r, &format!("target_{axis}"))?
                                - number(r, &format!("from_{axis}"))?;
                        }
                        let norm = vector[0].hypot(vector[1]).hypot(vector[2]);
                        if !norm.is_finite() || norm == 0.0 {
                            return Err(
                                "Contact has no finite commanded approach direction.".into()
                            );
                        }
                        vector.map(|v| v / norm)
                    } else {
                        let axis = number(r, "axis")?;
                        let sign = number(r, "direction")?;
                        if !matches!(axis, 0.0 | 1.0 | 2.0) || !matches!(sign, -1.0 | 1.0) {
                            return Err("Contact axis or approach direction is invalid.".into());
                        }
                        let mut vector = [0.0; 3];
                        vector[axis as usize] = sign;
                        vector
                    };
                    let commanded_feed_mm_min = number(r, "feed")?;
                    if commanded_feed_mm_min <= 0.0 {
                        return Err("Contact commanded feed is not positive.".into());
                    }
                    contacts.push(Contact {
                        sequence,
                        stage,
                        trigger_mm,
                        direction,
                        commanded_feed_mm_min,
                    });
                }
                _ => (), // Other schema-validated records remain in the raw ledger.
            }
        }
        let state = if quarantined {
            CaptureState::Quarantined
        } else if result {
            CaptureState::ResultUnreviewed
        } else {
            CaptureState::Partial
        };
        Ok(Self {
            workflow,
            state,
            records,
            contacts,
            misses,
            nominal_ball_diameter_mm,
        })
    }

    pub fn summary(&self) -> String {
        format!("\"workflow\":{},\"state\":{},\"records\":{},\"coarse_contacts\":{},\"fine_contacts\":{},\"misses\":{},\"nominal_ball_diameter_mm\":{},\"coordinate_frame\":\"linuxcnc-machine-trigger\",\"units\":\"mm\",\"ball_radius_compensation_applied\":false,\"mounting_offset_calibrated\":false,\"uncertainty_mm\":null",
            quote(self.workflow.name()), quote(self.state.name()), self.records.len(),
            self.contacts.iter().filter(|p| p.stage == Stage::Coarse).count(),
            self.contacts.iter().filter(|p| p.stage == Stage::Fine).count(), self.misses, self.nominal_ball_diameter_mm)
    }
}
