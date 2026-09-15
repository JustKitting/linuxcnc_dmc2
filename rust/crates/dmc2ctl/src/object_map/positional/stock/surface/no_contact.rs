//! Source-bound no-contact constraints, never invented surface observations.
mod capture;
mod geometry;
#[cfg(test)]
mod tests;
use super::{
    geometry::*,
    request::{NoContactModel, NoContactSources, Request},
};
use crate::object_map::positional::{mesh::Triangle, probe::Sample};
use crate::object_map::{
    capture_selection::{Entry, Use},
    model::Id,
    record::quote,
    store::CaptureSnapshot,
    Error,
};

pub enum Issue {
    Exclusion,
    Conflict,
}
impl Issue {
    pub fn description(self) -> (&'static str, &'static str) {
        match self {
            Self::Exclusion => (
                "retained-no-contact-exclusion",
                "The projected patch lies inside an original no-contact path under the source analysis's declared probe/error model. Inspect the retained miss and contact records or acquire the gap boundary; no surface was filled through it.",
            ),
            Self::Conflict => (
                "contact-no-contact-model-conflict",
                "A retained contact, corrected using the fitted patch normal, lies inside a retained eroded no-contact sweep. Resolve the capture reference, probe/error model or local normal with both original records retained; do not discard either observation or use this patch for material support.",
            ),
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
        format!(
            "{{\"source\":{},\"position_source\":\"reported travel, not a trigger\",\"from_machine_mm\":{},\"reported_end_machine_mm\":{},\"requested_end_machine_mm\":{},\"center_from_machine_mm\":{},\"center_end_machine_mm\":{},\"eroded_ball_radius_mm\":{},\"feed_mm_min\":{}}}",
            self.reference(),
            json(self.from),
            json(self.reported_end),
            json(self.requested_end),
            json(self.center_from),
            json(self.center_end),
            self.radius,
            self.feed
        )
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
    pub fn ball_separation(&self, p: V, radius: f64) -> Result<f64, Error> {
        geometry::point_segment(p, self.center_from, self.center_end)
            .map(|d| d - self.radius - radius)
            .filter(|d| d.is_finite())
            .ok_or_else(|| Error::Data(format!("Support separation from {}:{} is nonfinite. Inspect source coordinates and the retained probe model before refining placement.", self.capture.as_str(), self.sequence)))
    }
    pub fn overlaps_triangle(&self, triangle: Triangle) -> Result<bool, Error> {
        self.within(
            geometry::segment_triangle(self.center_from, self.center_end, triangle),
            self.radius,
        )
    }
    pub fn triangle_distance(&self, triangle: Triangle) -> Result<f64, Error> {
        geometry::segment_triangle(self.center_from, self.center_end, triangle)
            .ok_or_else(|| Error::Data(format!("No-contact distance for {}:{} cannot be evaluated with finite arithmetic. Inspect the retained geometry and coordinate units before reassessing; no empty region was inferred.",self.capture.as_str(),self.sequence)))
    }
}

pub fn prepare(captures: &[CaptureSnapshot]) -> Vec<Entry> {
    captures.iter().map(|c| {
        let (usage, reason) = match capture::read(c) {
            Ok(_) => (Use::Include, "Complete coarse-miss cycles with original acquisition context; review the shared frame and probe/error model.".into()),
            Err(e) => (Use::Exclude, e.to_string()),
        };
        Entry { capture: c.id.clone(), usage, reason }
    }).collect()
}

pub fn read(
    captures: &[CaptureSnapshot],
    samples: &[Sample],
    r: &Request,
) -> Result<Vec<Sweep>, Error> {
    let NoContactModel::ErodedProbeSweep { allowance } = r.no_contact else {
        return Ok(Vec::new());
    };
    let selected = match &r.no_contact_sources {
        NoContactSources::ContributingContacts => captures.iter().filter(|c| samples.iter().any(|s| s.capture == c.id) && c.capture.records.iter().any(|r| r["kind"] == "miss")).collect::<Vec<_>>(),
        NoContactSources::Explicit(entries) => entries.iter().filter(|e| e.usage == Use::Include).map(|e| {
            captures.iter().find(|c| c.id == e.capture).ok_or_else(|| Error::Data(format!("No-contact capture {} is absent from this setup. Import its original ledger and companions or correct the explicit selection before retrying.",e.capture.as_str())))
        }).collect::<Result<Vec<_>, _>>()?,
    };
    let radius = r.probe.radius - r.probe.pretravel - allowance;
    let mut result = Vec::new();
    for c in selected {
        for path in capture::read(c)? {
            let center_from = add(path.from, r.probe.mount);
            let center_end = add(path.reported_end, r.probe.mount);
            if ![center_from, center_end].into_iter().all(finite)
                || !norm(sub(center_end, center_from)).is_finite()
                || center_from == center_end
            {
                return Err(Error::Data(format!(
                    "{}:{} has a zero-length or unrepresentable no-contact path after frame correction. Inspect units and the retained positions before reuse.",
                    c.id.as_str(),
                    path.sequence
                )));
            }
            result.push(Sweep {
                capture: c.id.clone(),
                sequence: path.sequence,
                from: path.from,
                reported_end: path.reported_end,
                requested_end: path.requested_end,
                center_from,
                center_end,
                radius,
                feed: path.feed,
            });
        }
    }
    Ok(result)
}
