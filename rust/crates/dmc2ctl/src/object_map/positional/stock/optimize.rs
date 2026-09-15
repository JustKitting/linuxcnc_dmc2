//! Shared bounded maximization; objectives provide geometry-specific bounds.
use crate::object_map::Error;
use std::{cmp::Ordering, collections::BinaryHeap};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Clearance,
    Resolution,
    Budget,
    FloatLimit,
}
pub struct Domain<const N: usize> {
    pub lo: [f64; N],
    pub hi: [f64; N],
    pub margin: f64,
    pub resolution: f64,
    pub evaluations: usize,
}
pub trait Objective<const N: usize> {
    fn value(&self, at: [f64; N]) -> Result<f64, Error>;
    /// Per-coordinate movement for splitting, and a bound for their combination.
    fn movement(&self, lo: [f64; N], hi: [f64; N]) -> ([f64; N], f64);
    fn upper(&self, value: f64, lo: [f64; N], hi: [f64; N]) -> f64 {
        value + self.movement(lo, hi).1
    }
}
struct Cell<const N: usize> {
    lo: [f64; N],
    hi: [f64; N],
    at: [f64; N],
    value: f64,
    upper: f64,
    id: usize,
}
impl<const N: usize> PartialEq for Cell<N> {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl<const N: usize> Eq for Cell<N> {}
impl<const N: usize> PartialOrd for Cell<N> {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl<const N: usize> Ord for Cell<N> {
    fn cmp(&self, o: &Self) -> Ordering {
        self.upper
            .total_cmp(&o.upper)
            .then_with(|| o.id.cmp(&self.id))
    }
}
pub struct Fitted<const N: usize> {
    pub at: [f64; N],
    pub clearance: f64,
    pub upper: f64,
    pub evaluations: usize,
    pub stop: Stop,
    pub history: Vec<(usize, [f64; N], f64)>,
}

fn cell<const N: usize>(
    lo: [f64; N],
    hi: [f64; N],
    id: usize,
    objective: &impl Objective<N>,
) -> Result<Cell<N>, Error> {
    let at = std::array::from_fn(|i| lo[i] + (hi[i] - lo[i]) / 2.);
    let value = objective.value(at)?;
    let upper = objective.upper(value, lo, hi);
    if !value.is_finite() || !upper.is_finite() || upper < value {
        return Err(Error::Input("Placement objective or movement bound is nonfinite/inconsistent. Inspect source units and restrict the numerical search domain.".into()));
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
pub fn run<const N: usize>(
    objective: &impl Objective<N>,
    r: &Domain<N>,
) -> Result<Fitted<N>, Error> {
    run_seeded(objective, r, None)
}
pub fn run_seeded<const N: usize>(
    objective: &impl Objective<N>,
    r: &Domain<N>,
    seed: Option<[f64; N]>,
) -> Result<Fitted<N>, Error> {
    let first = cell(r.lo, r.hi, 0, objective)?;
    let mut best = (first.at, first.value);
    let mut evaluations = 1usize;
    let mut history = vec![(evaluations, best.0, best.1)];
    if let Some(at) = seed {
        if (0..N).any(|i| !at[i].is_finite() || at[i] < r.lo[i] || at[i] > r.hi[i]) {
            return Err(Error::Input("The retained initial placement lies outside the refinement domain. Include the initial placement in every requested interval.".into()));
        }
        if at != first.at {
            if r.evaluations <= evaluations {
                return Err(Error::Input("The computation budget cannot evaluate both the retained placement and the domain midpoint. Increase max_evaluations before refining placement.".into()));
            }
            let value = objective.value(at)?;
            if !value.is_finite() {
                return Err(Error::Data("The retained placement's objective is nonfinite. Inspect source geometry and units before refinement.".into()));
            }
            evaluations += 1;
            if value >= best.1 {
                best = (at, value);
                history.push((evaluations, at, value));
            }
        }
    }
    let mut queue = BinaryHeap::from([first]);
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
        let m = objective.movement(current.lo, current.hi).0;
        let i = (0..N).max_by(|a, b| m[*a].total_cmp(&m[*b])).unwrap();
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
            let next = cell(lo, hi, evaluations, objective)?;
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

/// Rigid movement bound for points no farther than radius from the origin.
pub fn rigid_movement(radius: f64, lo: [f64; 6], hi: [f64; 6]) -> ([f64; 6], f64) {
    let half = std::array::from_fn::<_, 6, _>(|i| (hi[i] - lo[i]) / 2.);
    let m = std::array::from_fn::<_, 6, _>(|i| {
        if i < 3 {
            half[i]
        } else {
            2. * radius * (half[i].min(std::f64::consts::PI) / 2.).sin()
        }
    });
    let total = m[0].hypot(m[1]).hypot(m[2]) + (m[3] + m[4] + m[5]).min(2. * radius);
    (m, total)
}
