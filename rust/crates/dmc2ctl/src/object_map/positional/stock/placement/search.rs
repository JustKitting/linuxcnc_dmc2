//! Bounded translation/yaw search for worst-case horizontal clearance.
use super::super::super::{Error, geometry::*};
use super::super::optimize::{self, Domain, Objective};
use super::{cover::Sample, polygon::Polygon, request::Request};
pub use optimize::Stop;
pub type Fitted = optimize::Fitted<3>;
pub fn description(stop: Stop) -> (&'static str, &'static str) {
    match stop {
        Stop::Clearance => (
            "footprint-clearance-candidate",
            "The covered mesh projection meets the requested clearance inside the estimated polygon. Inspect the candidate and unresolved height/volume before reuse.",
        ),
        Stop::Resolution => (
            "footprint-search-resolution",
            "The remaining search bound is within the requested resolution. Inspect deficit and coverage bounds; this does not establish three-dimensional containment.",
        ),
        Stop::Budget => (
            "footprint-search-budget",
            "The numerical search exhausted its evaluation budget. The best candidate and remaining bound are retained; edit the budget or search region and use a new analysis ID.",
        ),
        Stop::FloatLimit => (
            "footprint-search-float-limit",
            "The remaining search cells cannot be subdivided at these coordinate scales. Inspect placement units and resolution before reuse.",
        ),
    }
}
pub fn pose(p: V, z: f64) -> Pose {
    Pose::from_euler([0., 0., p[2].to_degrees()], [p[0], p[1], z])
}
pub fn score(p: V, z: f64, polygon: &Polygon, samples: &[Sample]) -> Result<f64, Error> {
    let t = pose(p, z);
    let mut clearance = f64::INFINITY;
    for s in samples {
        let d = polygon.distance(t.point(s.center))?.0;
        clearance = clearance.min(-d - s.radius);
    }
    if !clearance.is_finite() {
        return Err(Error::Data("The footprint objective has no finite covered geometry; inspect the source mesh and query bounds.".into()));
    }
    Ok(clearance)
}

struct Footprint<'a> {
    polygon: &'a Polygon,
    samples: &'a [Sample],
    z: f64,
    radius: f64,
}
impl Objective<3> for Footprint<'_> {
    fn value(&self, at: V) -> Result<f64, Error> {
        score(at, self.z, self.polygon, self.samples)
    }
    fn upper(&self, value: f64, lo: V, hi: V) -> f64 {
        let m = self.movement(lo, hi).0;
        // Preserve the original footprint addition order and retained bounds.
        value + m[0].hypot(m[1]) + m[2]
    }
    fn movement(&self, lo: V, hi: V) -> (V, f64) {
        let half = std::array::from_fn::<_, 3, _>(|i| (hi[i] - lo[i]) / 2.);
        let m = [
            half[0],
            half[1],
            2. * self.radius * (half[2].min(std::f64::consts::PI) / 2.).sin(),
        ];
        (m, m[0].hypot(m[1]) + m[2])
    }
}
pub fn run(poly: &Polygon, samples: &[Sample], r: &Request) -> Result<Fitted, Error> {
    let radius = samples
        .iter()
        .map(|s| s.center[0].hypot(s.center[1]))
        .fold(0_f64, f64::max);
    optimize::run(
        &Footprint {
            polygon: poly,
            samples,
            z: r.z,
            radius,
        },
        &Domain {
            lo: r.lo,
            hi: r.hi,
            margin: r.margin,
            resolution: r.resolution,
            evaluations: r.evaluations,
        },
    )
}
