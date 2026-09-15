use super::super::super::{cover, geometry::*, probe::Sample, request::Use, Error};
use super::super::fit::Stop;
use super::{request::Request, surface};
use surface::{support::Support, Patch, Station};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Unsupported,
    Unchecked,
    CheckConflict,
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
            Self::Shortage => ("local-clearance-shortage", "The required geometry lacks requested clearance under a checked local plane model. Inspect the source contacts and independent remeasurement before changing the candidate placement."),
            Self::BoundaryBand => ("local-clearance-bound-unresolved", "The cover/allowance interval crosses the requested clearance. Refine computational coverage or the measured surface allowance using evidence, then reassess."),
            Self::LocallyInward => ("locally-inward-only", "This region meets local checked-plane clearance. Closed boundary coverage, enclosed material and cavities remain unresolved; do not treat local inwardness as a solid."),
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Checks {
    Missing,
    Disagrees,
    Within,
}
impl Checks {
    pub fn name(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Disagrees => "disagrees",
            Self::Within => "within-requested-residual",
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
}
struct Local<'a> {
    station: &'a Station,
    patch: &'a Patch,
    support: Support,
    checks: Checks,
    check_sources: Vec<usize>,
}
pub fn run(
    cover: &[cover::Sample],
    samples: &[Sample],
    stations: &[Station],
    sr: &surface::request::Request,
    pose: Pose,
    r: &Request,
) -> Result<Vec<Region>, Error> {
    let required = stations
        .len()
        .checked_mul(cover.len().saturating_add(samples.len()));
    if required.is_none_or(|n| n > r.comparisons) {
        return Err(Error::Input("max_patch_comparisons cannot cover every selected patch, check and triangle region. Increase this computational budget or refine the explicitly selected source analyses; no region was dropped.".into()));
    }
    let mut local = Vec::new();
    for station in stations {
        let Ok(patch) = &station.result else { continue };
        if patch.stop != Stop::Converged
            || patch.residuals.iter().any(|d| d.abs() > sr.max_residual)
        {
            continue;
        }
        let support = Support::new(samples, station, patch, sr);
        let check_sources = samples
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.usage == Use::Check
                    && dot(s.approach, patch.normal) < 0.
                    && support.contains(s.center, patch, sr)
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let checks = if check_sources.is_empty() {
            Checks::Missing
        } else if check_sources.iter().any(|i| {
            dot(sub(samples[*i].center, patch.center), patch.normal).abs() > sr.max_residual
        }) {
            Checks::Disagrees
        } else {
            Checks::Within
        };
        local.push(Local {
            station,
            patch,
            support,
            checks,
            check_sources,
        });
    }
    let mut result = Vec::with_capacity(cover.len());
    for sample in cover {
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
            if d.abs() + sample.radius > r.band || !l.support.covers(p, sample.radius, l.patch, sr)
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
        let state = if comparisons.is_empty() {
            State::Unsupported
        } else if comparisons.iter().any(|c| c.checks == Checks::Disagrees) {
            State::CheckConflict
        } else if checked.is_empty() {
            State::Unchecked
        } else if checked.iter().any(|c| c.upper < r.clearance) {
            State::Shortage
        } else if checked.iter().any(|c| c.lower < r.clearance) {
            State::BoundaryBand
        } else {
            State::LocallyInward
        };
        result.push(Region { state, comparisons });
    }
    Ok(result)
}
