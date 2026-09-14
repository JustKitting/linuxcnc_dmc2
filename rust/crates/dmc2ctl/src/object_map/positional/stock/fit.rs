//! Local robust orthogonal regression along an ordered, unrestricted XY contour.
//! It estimates the ball-centre locus; wall slope is not inferred from a slice.
use super::{
    super::{geometry::*, probe::Sample, request::Use, Error},
    request::{Closure, Request, Surface},
};
use std::collections::BTreeSet;
type P = [f64; 2];
fn dot2(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn sub2(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}
fn length(a: P) -> f64 {
    a[0].hypot(a[1])
}
fn xy(p: V) -> P {
    [p[0], p[1]]
}
pub fn distance(a: V, b: V) -> f64 {
    length(sub2(xy(a), xy(b)))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Converged,
    IterationLimit,
    NoDescent,
}
impl Stop {
    pub fn name(self) -> &'static str {
        match self {
            Self::Converged => "local-step-tolerance",
            Self::IterationLimit => "iteration-budget-exhausted",
            Self::NoDescent => "no-decreasing-step",
        }
    }
}
pub struct Station {
    pub sample: usize,
    pub neighbours: Vec<usize>,
    pub center: V,
    pub surface: Option<V>,
    pub normal: V,
    pub residuals: Vec<f64>,
    pub weights: Vec<f64>,
    pub iterations: usize,
    pub stop: Stop,
    pub objective_start: f64,
    pub objective_end: f64,
}
pub struct Contour {
    pub stations: Vec<Station>,
}
#[derive(Clone, Copy)]
struct Line {
    normal: P,
    origin: P,
}
impl Line {
    fn residual(self, p: P) -> f64 {
        dot2(self.normal, sub2(p, self.origin))
    }
    fn project(self, p: P) -> P {
        let r = self.residual(p);
        [p[0] - r * self.normal[0], p[1] - r * self.normal[1]]
    }
}
fn line(points: &[P], weights: &[f64]) -> Result<Line, Error> {
    let sum = weights.iter().sum::<f64>();
    let origin = std::array::from_fn(|i| {
        points
            .iter()
            .zip(weights)
            .map(|(p, w)| p[i] * (w / sum))
            .sum()
    });
    let (mut a, mut b, mut c) = (0., 0., 0.);
    for (p, w) in points.iter().zip(weights) {
        let d = sub2(*p, origin);
        let w = w / sum;
        a += w * d[0] * d[0];
        b += w * d[0] * d[1];
        c += w * d[1] * d[1];
    }
    // Covariance is dimensionless after scaling. This detects numerical rank,
    // not an accuracy tolerance. Isotropic points do not define a tangent.
    let gap = (a - c).hypot(2. * b);
    if !gap.is_finite() || gap <= f64::EPSILON * (points.len() as f64) * (a + c) {
        return Err(Error::Data("A rim neighbourhood has no distinguishable tangent. Select a supported local span or gather more directional contacts; no arbitrary normal was assigned.".into()));
    }
    let angle = (2. * b).atan2(a - c) / 2.;
    Ok(Line {
        normal: [-angle.sin(), angle.cos()],
        origin,
    })
}
fn loss(r: f64, h: f64) -> f64 {
    if r.abs() <= h {
        r * r / 2.
    } else {
        h * (r.abs() - h / 2.)
    }
}
fn local(
    samples: &[Sample],
    target: usize,
    neighbours: Vec<usize>,
    r: &Request,
) -> Result<Station, Error> {
    let anchor = samples[target].center;
    let mut points = neighbours
        .iter()
        .map(|i| sub2(xy(samples[*i].center), xy(anchor)))
        .collect::<Vec<_>>();
    let extent = points.iter().map(|p| length(*p)).fold(0_f64, f64::max);
    if extent == 0. || !extent.is_finite() {
        return Err(Error::Data(
            "A rim neighbourhood has no finite XY extent. Retain distinct neighbouring contacts."
                .into(),
        ));
    }
    for p in &mut points {
        for v in p {
            *v /= extent;
        }
    }
    let h = r.huber / extent;
    if h == 0. || !h.is_finite() {
        return Err(Error::Data("The robust scale cannot be represented at this contour extent; check units and fit scale.".into()));
    }
    let mut current = line(&points, &vec![1.; points.len()])?;
    let objective = |l: Line| points.iter().map(|p| loss(l.residual(*p), h)).sum::<f64>();
    let start = objective(current);
    let mut old = start;
    let mut stop = Stop::IterationLimit;
    let mut iterations = 0;
    for step in 0..r.iterations {
        let weights = points
            .iter()
            .map(|p| {
                let d = current.residual(*p).abs();
                if d <= h {
                    1.
                } else {
                    h / d
                }
            })
            .collect::<Vec<_>>();
        let next = line(&points, &weights)?;
        let value = objective(next);
        let roundoff = f64::EPSILON * (points.len() as f64) * old.max(value);
        iterations = step + 1;
        if value > old + roundoff {
            stop = Stop::NoDescent;
            break;
        }
        let change = points
            .iter()
            .map(|p| length(sub2(current.project(*p), next.project(*p))) * extent)
            .fold(0_f64, f64::max);
        current = next;
        old = value;
        if change <= r.convergence {
            stop = Stop::Converged;
            break;
        }
    }
    let approach = xy(samples[target].approach);
    let facing = dot2(current.normal, approach);
    if facing == 0. || !facing.is_finite() {
        return Err(Error::Data(format!("Rim contact {}:{} has no approach component across its estimated XY edge. Select side contacts or refine that region; no outward direction was invented.",samples[target].capture.as_str(),samples[target].sequence)));
    }
    if facing > 0. {
        current.normal = current.normal.map(|x| -x);
    }
    let p = current.project([0., 0.]);
    let center = [
        anchor[0] + p[0] * extent,
        anchor[1] + p[1] * extent,
        anchor[2],
    ];
    let normal = [current.normal[0], current.normal[1], 0.];
    let surface =
        (r.surface == Surface::VerticalSides).then(|| sub(center, scale(normal, r.probe.radius)));
    let residuals = points
        .iter()
        .map(|p| current.residual(*p) * extent)
        .collect::<Vec<_>>();
    let weights = residuals
        .iter()
        .map(|x| {
            if x.abs() <= r.huber {
                1.
            } else {
                r.huber / x.abs()
            }
        })
        .collect();
    let objective_start = (start * extent) * extent;
    let objective_end = (old * extent) * extent;
    if !finite(center)
        || surface.is_some_and(|p| !finite(p))
        || residuals.iter().any(|v| !v.is_finite())
        || !objective_start.is_finite()
        || !objective_end.is_finite()
    {
        return Err(Error::Data(
            "Stock contour arithmetic overflowed; check measurement units and fit scale.".into(),
        ));
    }
    Ok(Station {
        sample: target,
        neighbours,
        center,
        surface,
        normal,
        residuals,
        weights,
        iterations,
        stop,
        objective_start,
        objective_end,
    })
}
pub fn run(samples: &[Sample], r: &Request) -> Result<Contour, Error> {
    let order = samples
        .iter()
        .enumerate()
        .filter_map(|(i, s)| (s.usage == Use::Fit).then_some(i))
        .collect::<Vec<_>>();
    let mut stations = Vec::new();
    for (at, target) in order.iter().enumerate() {
        let mut neighbours = BTreeSet::from([*target]);
        for forward in [false, true] {
            let mut previous = at;
            let mut travelled = 0.;
            for step in 1..order.len() {
                if r.closure == Closure::Open
                    && ((forward && at + step >= order.len()) || (!forward && step > at))
                {
                    break;
                }
                let next = if forward {
                    (at + step) % order.len()
                } else {
                    (at + order.len() - step) % order.len()
                };
                let gap = distance(samples[order[previous]].center, samples[order[next]].center);
                if !gap.is_finite() {
                    return Err(Error::Data(
                        "A rim gap overflows; inspect capture coordinates.".into(),
                    ));
                }
                travelled += gap;
                if gap > r.max_gap || travelled > r.span {
                    break;
                }
                neighbours.insert(order[next]);
                previous = next;
            }
        }
        stations.push(local(
            samples,
            *target,
            neighbours.into_iter().collect(),
            r,
        )?);
    }
    Ok(Contour { stations })
}
