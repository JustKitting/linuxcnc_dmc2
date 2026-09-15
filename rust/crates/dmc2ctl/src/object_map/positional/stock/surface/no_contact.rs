//! Source-bound no-contact constraints, never invented surface observations.
mod geometry;
#[cfg(test)]
mod tests;
use super::{
    geometry::*,
    request::{NoContactModel, Request},
};
use crate::object_map::positional::{mesh::Triangle, probe::Sample};
use crate::object_map::{
    model::{CaptureState, Id},
    record::quote,
    store::CaptureSnapshot,
    Error,
};
use crate::probe_data::{
    ledger::number,
    mapper_settings::{close, xyz},
    mapper_trace::state,
    schema::Workflow,
};

pub enum Issue {
    Exclusion,
    Conflict,
}
impl Issue {
    pub fn description(self) -> (&'static str, &'static str) {
        match self {
            Self::Exclusion => ("retained-no-contact-exclusion", "The projected patch lies inside an original no-contact path under the source analysis's declared probe/error model. Inspect the retained miss and contact records or acquire the gap boundary; no surface was filled through it."),
            Self::Conflict => ("contact-no-contact-model-conflict", "A retained contact, corrected using the fitted patch normal, lies inside a retained eroded no-contact sweep. Resolve the capture reference, probe/error model or local normal with both original records retained; do not discard either observation or use this patch for material support."),
        }
    }
}

#[derive(Clone)]
pub struct Sweep {
    pub capture: Id,
    pub sequence: usize,
    pub from: V,
    pub reported_end: V,
    pub requested_end: V,
    pub center_from: V,
    pub center_end: V,
    pub radius: f64,
    pub feed: f64,
}
impl Sweep {
    pub fn reference(&self) -> String {
        format!(
            "{{\"capture\":{},\"sequence\":{}}}",
            quote(self.capture.as_str()),
            self.sequence
        )
    }
    pub fn json(&self) -> String {
        format!("{{\"source\":{},\"position_source\":\"reported travel, not a trigger\",\"from_machine_mm\":{},\"reported_end_machine_mm\":{},\"requested_end_machine_mm\":{},\"center_from_machine_mm\":{},\"center_end_machine_mm\":{},\"eroded_ball_radius_mm\":{},\"feed_mm_min\":{}}}", self.reference(), json(self.from), json(self.reported_end), json(self.requested_end), json(self.center_from), json(self.center_end), self.radius, self.feed)
    }
    fn within(&self, distance: Option<f64>, radius: f64) -> Result<bool, Error> {
        let d = distance.filter(|_| radius.is_finite()).ok_or_else(|| Error::Data(format!("No-contact geometry for {}:{} cannot be evaluated with finite arithmetic. Inspect coordinate units, probe correction and the analysis scale; no supported surface was substituted.",self.capture.as_str(),self.sequence)))?;
        Ok(d <= radius)
    }
    pub fn excludes_point(&self, p: V) -> Result<bool, Error> {
        self.within(
            geometry::point_segment(p, self.center_from, self.center_end),
            self.radius,
        )
    }
    pub fn overlaps_ball(&self, p: V, radius: f64) -> Result<bool, Error> {
        self.within(
            geometry::point_segment(p, self.center_from, self.center_end),
            self.radius + radius,
        )
    }
    pub fn overlaps_triangle(&self, triangle: Triangle) -> Result<bool, Error> {
        self.within(
            geometry::segment_triangle(self.center_from, self.center_end, triangle),
            self.radius,
        )
    }
}

pub fn read(
    captures: &[CaptureSnapshot],
    samples: &[Sample],
    r: &Request,
) -> Result<Vec<Sweep>, Error> {
    let NoContactModel::ErodedProbeSweep { allowance } = r.no_contact else {
        return Ok(Vec::new());
    };
    let radius = r.probe.radius - r.probe.pretravel - allowance;
    let mut result = Vec::new();
    for c in captures
        .iter()
        .filter(|c| samples.iter().any(|s| s.capture == c.id))
    {
        let records: Vec<_> = c
            .capture
            .records
            .iter()
            .filter(|r| r["kind"] == "miss")
            .collect();
        if records.is_empty() {
            continue;
        }
        if c.capture.state == CaptureState::Quarantined || c.capture.workflow != Workflow::Mapper {
            return Err(Error::Data(format!("Capture {} has no-contact records without a supported, non-quarantined mapper cycle. Preserve it and select a mapper capture with original acquisition context; no empty-space constraint was inferred.",c.id.as_str())));
        }
        let s = c.context.settings(&c.capture).map_err(Error::Data)?;
        let retained = state::samples(&c.capture.records, &s, false).map_err(Error::Data)?;
        for record in records {
            let sequence = record["sequence"].parse::<usize>().map_err(|_| Error::Data("A no-contact record has an invalid identity. Preserve the source and import an intact capture.".into()))?;
            let sample = retained.iter().find(|sample| sample.sequence == sequence && sample.trigger.is_none())
                .ok_or_else(|| Error::Data(format!("{}:{sequence} is not a retained complete coarse-miss cycle. Preserve its diagnosis and recapture after operator recovery.",c.id.as_str())))?;
            let from = xyz(record, "from_", "").map_err(Error::Data)?;
            let end = xyz(record, "work_", "").map_err(Error::Data)?;
            let feed = number(record, "feed").map_err(Error::Data)?;
            if !s.endpoint_matches(end, sample.request.target)
                || !close(feed, s.coarse_feed(sample.request.phase))
            {
                return Err(Error::Data(format!("{}:{sequence} lacks a reported endpoint and feed matching its retained coarse search. No full-travel no-contact constraint was accepted; preserve and inspect this capture.",c.id.as_str())));
            }
            s.bounds(from).map_err(Error::Data)?;
            s.bounds(end).map_err(Error::Data)?;
            let from = add(from, s.offset);
            let reported_end = add(end, s.offset);
            let requested_end = add(sample.request.target, s.offset);
            let center_from = add(from, r.probe.mount);
            let center_end = add(reported_end, r.probe.mount);
            if ![from, reported_end, requested_end, center_from, center_end]
                .into_iter()
                .all(finite)
                || !norm(sub(center_end, center_from)).is_finite()
                || center_from == center_end
            {
                return Err(Error::Data(format!("{}:{sequence} has a zero-length or unrepresentable no-contact path after frame correction. Inspect units and the retained positions before reuse.", c.id.as_str())));
            }
            result.push(Sweep {
                capture: c.id.clone(),
                sequence,
                from,
                reported_end,
                requested_end,
                center_from,
                center_end,
                radius,
                feed,
            });
        }
    }
    Ok(result)
}
