use super::super::super::{cover, geometry::*, probe::Sample, Error};
use super::{empty, request::Request, surface};
pub(super) use surface::local::Checks;
use surface::Station;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Unsupported,
    Unchecked,
    CheckConflict,
    NoContactConflict,
    EmptyOverlap,
    EmptyBoundary,
    Shortage,
    BoundaryBand,
    LocallyInward,
}
impl State {
    pub fn description(self) -> (&'static str, &'static str) {
        match self {
            Self::Unsupported => ("unobserved-local-coverage", "Acquire surface support around this required-geometry region; inspect the retained source's fit, support and normal band. This coordinate is a region of interest, not a probe endpoint."),
            Self::Unchecked => ("independent-support-check-missing", "Retain independent contacts checking these local patches before relying on their material comparison."),
            Self::CheckConflict => ("independent-support-check-disagreement", "Resolve the retained independent check disagreement before using these local material comparisons; no failing contact was removed."),
            Self::NoContactConflict => surface::no_contact::Issue::Conflict.description(),
            Self::EmptyOverlap => ("required-geometry-no-contact-overlap", "An actual required triangle fragment intersects retained clear travel under the declared eroded-probe model. Review the original miss, setup/probe reference and required geometry before changing placement; nearby top contacts cannot fill this recorded gap."),
            Self::EmptyBoundary => ("required-geometry-no-contact-boundary", "An actual required triangle fragment touches the boundary of a retained eroded-probe sweep. Resolve this model boundary using the retained uncertainty and geometry; no positive material coverage or penetration was inferred."),
            Self::Shortage => ("local-clearance-shortage", "The required geometry lacks requested clearance under a checked local plane model. Inspect the source contacts and independent remeasurement before changing the candidate placement."),
            Self::BoundaryBand => ("local-clearance-bound-unresolved", "The cover/allowance interval crosses the requested clearance. Refine computational coverage or the measured surface allowance using evidence, then reassess."),
            Self::LocallyInward => ("locally-inward-only", "This region meets local checked-plane clearance. Closed boundary coverage, enclosed material and cavities remain unresolved; do not treat local inwardness as a solid."),
        }
    }
}
pub struct Comparison {
    pub source: usize,
    pub checks: Checks,
    pub check_sources: Vec<usize>,
    pub distance: f64,
    pub lower: f64,
    pub upper: f64,
    pub projection: V,
    pub normal: V,
}
pub struct Region {
    pub state: State,
    pub comparisons: Vec<Comparison>,
    pub no_contact: Vec<empty::Overlap>,
}
pub fn run(
    cover: &[cover::Sample],
    samples: &[Sample],
    stations: &[Station],
    sr: &surface::request::Request,
    pose: Pose,
    r: &Request,
    misses: &[surface::no_contact::Sweep],
) -> Result<Vec<Region>, Error> {
    let required = stations
        .len()
        .checked_mul(cover.len().saturating_add(samples.len()));
    if required.is_none_or(|n| n > r.comparisons) {
        return Err(Error::Input("max_patch_comparisons cannot cover every selected patch, check and triangle region. Increase this computational budget or refine the explicitly selected source analyses; no region was dropped.".into()));
    }
    let local = surface::local::build(samples, stations, sr)?;
    let empty = empty::run(cover, &local, misses, sr, pose, r.empty)?;
    let mut result = Vec::with_capacity(cover.len());
    for (sample, no_contact) in cover.iter().zip(empty) {
        let p = pose.point(sample.center);
        if !finite(p) {
            return Err(Error::Data("Candidate coordinates overflowed during material comparison. Inspect units and the retained pose.".into()));
        }
        let mut comparisons = Vec::new();
        for l in &local {
            let d = dot(sub(p, l.patch.surface), l.patch.normal);
            if !d.is_finite() {
                return Err(Error::Data("Local signed distance overflowed. Inspect source coordinate units and the retained pose.".into()));
            }
            if d.abs() + sample.radius > r.band
                || !l.support.covers(p, sample.radius, l.patch, sr)?
            {
                continue;
            }
            // Every covered subtriangle includes its centroid. Its minimum
            // clearance is bounded below over the full covering ball, and
            // above by clearance at that centroid, including the stated allowance.
            let lower = -d - sample.radius - r.allowance;
            let upper = -d + r.allowance;
            let projection = sub(p, scale(l.patch.normal, d));
            if !lower.is_finite() || !upper.is_finite() || !finite(projection) {
                return Err(Error::Data(
                    "Material bound arithmetic overflowed. Inspect cover, allowance and units."
                        .into(),
                ));
            }
            comparisons.push(Comparison {
                source: l.station.seed,
                checks: l.checks,
                check_sources: l.check_sources.clone(),
                distance: d,
                lower,
                upper,
                projection,
                normal: l.patch.normal,
            });
        }
        let checked = comparisons
            .iter()
            .filter(|c| c.checks == Checks::Within)
            .collect::<Vec<_>>();
        let state = if comparisons
            .iter()
            .any(|c| c.checks == Checks::NoContactConflict)
            || no_contact.iter().any(|o| !o.source.conflicts.is_empty())
        {
            State::NoContactConflict
        } else if comparisons.iter().any(|c| c.checks == Checks::Disagrees) {
            State::CheckConflict
        } else if no_contact.iter().any(|o| o.separation < 0.) {
            State::EmptyOverlap
        } else if !no_contact.is_empty() {
            State::EmptyBoundary
        } else if comparisons.is_empty() {
            State::Unsupported
        } else if checked.is_empty() {
            State::Unchecked
        } else if checked.iter().any(|c| c.upper < r.clearance) {
            State::Shortage
        } else if checked.iter().any(|c| c.lower < r.clearance) {
            State::BoundaryBand
        } else {
            State::LocallyInward
        };
        result.push(Region {
            state,
            comparisons,
            no_contact,
        });
    }
    Ok(result)
}
