use super::super::super::{probe::Sample, request::Use};
use super::super::fit::Stop;
use super::{geometry::*, plane, request::Request, Patch, Reason, Station};
fn loss(d: f64, h: f64) -> f64 {
    if d.abs() <= h {
        d * d / 2.
    } else {
        h * (d.abs() - h / 2.)
    }
}
fn local(
    samples: &[Sample],
    seed: usize,
    neighbours: &[usize],
    r: &Request,
) -> Result<Patch, Reason> {
    if neighbours.len() < 3 {
        return Err(Reason::FewContacts);
    }
    let anchor = samples[seed].center;
    let mut points = neighbours
        .iter()
        .map(|i| sub(samples[*i].center, anchor))
        .collect::<Vec<_>>();
    let extent = points.iter().map(|p| norm(*p)).fold(0_f64, f64::max);
    if extent == 0. {
        return Err(Reason::UnobservedNormal);
    }
    if !extent.is_finite() {
        return Err(Reason::Arithmetic);
    }
    for p in &mut points {
        *p = p.map(|x| x / extent);
    }
    let h = r.huber / extent;
    if h == 0. || !h.is_finite() {
        return Err(Reason::Arithmetic);
    }
    let mut current = plane::fit(&points, &vec![1.; points.len()], r.iterations)?;
    let objective = |p: plane::Plane| points.iter().map(|x| loss(p.residual(*x), h)).sum::<f64>();
    let start = objective(current);
    let mut previous = start;
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
        let next = plane::fit(&points, &weights, r.iterations)?;
        let value = objective(next);
        if !value.is_finite() {
            return Err(Reason::Arithmetic);
        }
        iterations = step + 1;
        if value > previous + f64::EPSILON * (points.len() as f64) * previous.max(value) {
            stop = Stop::NoDescent;
            break;
        }
        let change = points
            .iter()
            .map(|p| norm(sub(current.project(*p), next.project(*p))) * extent)
            .fold(0_f64, f64::max);
        current = next;
        previous = value;
        if change <= r.convergence {
            stop = Stop::Converged;
            break;
        }
    }
    let facing = dot(current.normal, samples[seed].approach);
    if facing == 0. || !facing.is_finite() {
        return Err(Reason::Approach);
    }
    if facing > 0. {
        current.normal = scale(current.normal, -1.);
    }
    if neighbours
        .iter()
        .any(|i| dot(current.normal, samples[*i].approach) >= 0.)
    {
        return Err(Reason::Approach);
    }
    let center = add(anchor, scale(current.project([0.; 3]), extent));
    let surface = sub(center, scale(current.normal, r.probe.radius));
    let residuals = points
        .iter()
        .map(|p| current.residual(*p) * extent)
        .collect::<Vec<_>>();
    let weights = residuals
        .iter()
        .map(|d| {
            if d.abs() <= r.huber {
                1.
            } else {
                r.huber / d.abs()
            }
        })
        .collect();
    let variance = current.variance.map(|v| (v * extent) * extent);
    let start = (start * extent) * extent;
    let end = (previous * extent) * extent;
    if !finite(center)
        || !finite(surface)
        || !finite(variance)
        || !start.is_finite()
        || !end.is_finite()
        || residuals.iter().any(|v| !v.is_finite())
    {
        return Err(Reason::Arithmetic);
    }
    Ok(Patch {
        center,
        surface,
        normal: current.normal,
        variance,
        residuals,
        weights,
        stop,
        iterations,
        objective_start: start,
        objective_end: end,
    })
}
pub fn run(samples: &[Sample], r: &Request, misses: &[super::no_contact::Sweep]) -> Vec<Station> {
    let no_contact: std::sync::Arc<[super::no_contact::Sweep]> = misses.into();
    samples
        .iter()
        .enumerate()
        .filter(|(_, s)| s.usage == Use::Fit)
        .map(|(seed, s)| {
            let neighbours = samples
                .iter()
                .enumerate()
                .filter(|(_, other)| {
                    other.usage == Use::Fit
                        && norm(sub(s.center, other.center)) <= r.neighborhood
                        && dot(s.approach, other.approach) >= r.approach_cos
                })
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            let result = local(samples, seed, &neighbours, r);
            Station {
                seed,
                neighbours,
                result,
                no_contact: no_contact.clone(),
            }
        })
        .collect()
}
