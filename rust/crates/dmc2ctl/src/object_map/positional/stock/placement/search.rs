//! Bounded translation/yaw search for worst-case horizontal clearance.
use super::super::super::{geometry::*, Error};
use super::{cover::Sample, polygon::Polygon, request::Request};
use std::{cmp::Ordering, collections::BinaryHeap};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Clearance,
    Resolution,
    Budget,
    FloatLimit,
}
impl Stop {
    pub fn description(self) -> (&'static str, &'static str) {
        match self {
            Self::Clearance=>("footprint-clearance-candidate","The covered mesh projection meets the requested clearance inside the estimated polygon. Inspect the candidate and unresolved height/volume before reuse."),
            Self::Resolution=>("footprint-search-resolution","The remaining search bound is within the requested resolution. Inspect deficit and coverage bounds; this does not establish three-dimensional containment."),
            Self::Budget=>("footprint-search-budget","The numerical search exhausted its evaluation budget. The best candidate and remaining bound are retained; edit the budget or search region and use a new analysis ID."),
            Self::FloatLimit=>("footprint-search-float-limit","The remaining search cells cannot be subdivided at these coordinate scales. Inspect placement units and resolution before reuse."),
        }
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
struct Cell {
    lo: V,
    hi: V,
    at: V,
    value: f64,
    upper: f64,
    id: usize,
}
impl PartialEq for Cell {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl Eq for Cell {}
impl PartialOrd for Cell {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Cell {
    fn cmp(&self, o: &Self) -> Ordering {
        self.upper
            .total_cmp(&o.upper)
            .then_with(|| o.id.cmp(&self.id))
    }
}
fn movement(lo: V, hi: V, radius: f64) -> V {
    let half = std::array::from_fn::<_, 3, _>(|i| (hi[i] - lo[i]) / 2.);
    [
        half[0],
        half[1],
        2. * radius * (half[2].min(std::f64::consts::PI) / 2.).sin(),
    ]
}
fn cell(
    lo: V,
    hi: V,
    radius: f64,
    id: usize,
    r: &Request,
    poly: &Polygon,
    samples: &[Sample],
) -> Result<Cell, Error> {
    let at = std::array::from_fn(|i| lo[i] + (hi[i] - lo[i]) / 2.);
    let value = score(at, r.z, poly, samples)?;
    let m = movement(lo, hi, radius);
    // Signed polygon distance is 1-Lipschitz. Every centre moves by at most
    // the translation half-diagonal plus the yaw chord within this cell.
    let upper = value + m[0].hypot(m[1]) + m[2];
    if !upper.is_finite() {
        return Err(Error::Input(
            "Placement bounds overflow the clearance bound. Restrict the numerical search domain."
                .into(),
        ));
    }
    Ok(Cell {
        lo,
        hi,
        at,
        value,
        upper,
        id,
    })
}
pub struct Fitted {
    pub at: V,
    pub clearance: f64,
    pub upper: f64,
    pub evaluations: usize,
    pub stop: Stop,
    pub history: Vec<(usize, V, f64)>,
}
pub fn run(poly: &Polygon, samples: &[Sample], r: &Request) -> Result<Fitted, Error> {
    let radius = samples
        .iter()
        .map(|s| s.center[0].hypot(s.center[1]))
        .fold(0_f64, f64::max);
    let first = cell(r.lo, r.hi, radius, 0, r, poly, samples)?;
    let mut best = (first.at, first.value);
    let mut queue = BinaryHeap::from([first]);
    let mut evaluations = 1usize;
    let mut history = vec![(evaluations, best.0, best.1)];
    let (stop, upper) = loop {
        let upper = queue.peek().map_or(best.1, |c| c.upper.max(best.1));
        if best.1 >= r.margin {
            break (Stop::Clearance, upper);
        }
        if upper - best.1 <= r.resolution {
            break (Stop::Resolution, upper);
        }
        if r.evaluations - evaluations < 2 {
            break (Stop::Budget, upper);
        }
        let current = queue.pop().unwrap();
        let m = movement(current.lo, current.hi, radius);
        let i = (0..3).max_by(|a, b| m[*a].total_cmp(&m[*b])).unwrap();
        let middle = current.at[i];
        if middle == current.lo[i] || middle == current.hi[i] {
            break (Stop::FloatLimit, upper);
        }
        for high in [false, true] {
            let (mut lo, mut hi) = (current.lo, current.hi);
            if high {
                lo[i] = middle;
            } else {
                hi[i] = middle;
            }
            let next = cell(lo, hi, radius, evaluations, r, poly, samples)?;
            evaluations += 1;
            if next.value > best.1 {
                best = (next.at, next.value);
                history.push((evaluations, best.0, best.1));
            }
            if next.upper > best.1 {
                queue.push(next);
            }
        }
    };
    Ok(Fitted {
        at: best.0,
        clearance: best.1,
        upper,
        evaluations,
        stop,
        history,
    })
}
