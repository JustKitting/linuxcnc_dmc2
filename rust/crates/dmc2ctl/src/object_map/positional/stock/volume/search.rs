use super::super::{
    super::{cover::Sample, geometry::*, mesh::solid::Solid},
    optimize::{self, Domain, Objective, Stop},
};
use super::request::Request;
use crate::object_map::Error;
pub type Fitted = optimize::Fitted<6>;
pub fn pose(at: [f64; 6]) -> Pose {
    Pose::from_euler(
        [at[3].to_degrees(), at[4].to_degrees(), at[5].to_degrees()],
        [at[0], at[1], at[2]],
    )
}
pub fn description(stop: Stop) -> (&'static str, &'static str) {
    match stop {
        Stop::Clearance => (
            "volume-clearance-candidate",
            "The full covered required surface meets clearance within the explicitly enclosed stock model. Inspect source assumptions, residuals and setup constraints; physical placement and CAM remain unaccepted.",
        ),
        Stop::Resolution => (
            "volume-search-resolution",
            "The remaining objective bound is within the requested search resolution. Inspect the retained best candidate and deficits; the requested clearance was not reached.",
        ),
        Stop::Budget => (
            "volume-search-budget",
            "The search exhausted max_evaluations. Its best candidate and remaining bound are retained; refine the allowed region or computation budget in a new request.",
        ),
        Stop::FloatLimit => (
            "volume-search-float-limit",
            "A remaining placement cell cannot be subdivided at these coordinate scales. Inspect source units, bounds and numerical resolution before another analysis.",
        ),
    }
}
pub(super) struct Enclosed<'a, 'm> {
    pub stock: &'a Solid<'m>,
    pub samples: &'a [Sample],
    pub radius: f64,
    pub allowance: f64,
}
impl Objective<6> for Enclosed<'_, '_> {
    fn value(&self, at: [f64; 6]) -> Result<f64, Error> {
        let transform = pose(at);
        let mut lower = f64::INFINITY;
        for s in self.samples {
            lower = lower.min(
                self.stock.distance(transform.point(s.center))?.inward - s.radius - self.allowance,
            );
        }
        Ok(lower)
    }
    fn movement(&self, lo: [f64; 6], hi: [f64; 6]) -> ([f64; 6], f64) {
        // Signed distance is 1-Lipschitz; the shared Rz Ry Rx chord bound
        // covers combined translations and rotations without resizing stock.
        optimize::rigid_movement(self.radius, lo, hi)
    }
}
pub fn run(stock: &Solid<'_>, samples: &[Sample], r: &Request) -> Result<Fitted, Error> {
    // Reserve the complete search plus one final reporting pass. This bounds
    // exact solid-angle terms without skipping any source triangle or sample.
    let terms=r.evaluations.checked_add(1).and_then(|n|n.checked_mul(samples.len())).and_then(|n|n.checked_mul(stock.triangle_count())).ok_or_else(||Error::Input("The volume query budget overflows. Reduce the requested evaluations/cover size before running this analysis.".into()))?;
    if terms > r.winding_terms {
        return Err(Error::Input(format!(
            "This full search and report require a budget of {terms} solid-angle terms, exceeding max_winding_terms={}. Increase that explicit computation budget or reduce the evaluations/cover count; no triangles were omitted.",
            r.winding_terms
        )));
    }
    let radius = samples.iter().map(|s| norm(s.center)).fold(0_f64, f64::max);
    let objective = Enclosed {
        stock,
        samples,
        radius,
        allowance: r.allowance,
    };
    optimize::run(
        &objective,
        &Domain {
            lo: r.lo,
            hi: r.hi,
            margin: r.margin,
            resolution: r.resolution,
            evaluations: r.evaluations,
        },
    )
}
