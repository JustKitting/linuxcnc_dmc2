//! Local sphere-to-triangle registration with retained residuals and explicit bounds.
use super::{
    super::{
        model::{CaptureState, Id},
        Error,
    },
    geometry::*,
    mesh::{Mesh, Nearest},
    request::{Request, Use},
};
pub struct Sample {
    pub capture: Id,
    pub sequence: usize,
    pub usage: Use,
    pub state: CaptureState,
    pub trigger: V,
    pub center: V,
    pub approach: V,
    pub feed: f64,
}
pub struct Observation {
    pub center: V,
    pub near: Nearest,
    pub residual: f64,
    pub weight: f64,
    pub facing: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Converged,
    IterationLimit,
    NoDescent,
    SearchBound,
}
impl Outcome {
    pub fn message(self) -> &'static str {
        match self {
            Self::Converged => "Local numerical fitting stopped within the requested step tolerance. Inspect the retained residuals and independent checks; this is an unreviewed placement proposal.",
            Self::IterationLimit => "The requested iteration budget ended. Inspect residuals and the initial placement, then retry under a new analysis ID.",
            Self::NoDescent => "No decreasing numerical step was found. Inspect reference selection, calibration and the initial placement before another fit.",
            Self::SearchBound => "The requested placement search bound was reached. Inspect the seed and references; no bound was increased automatically.",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Converged => "numerically-converged-proposal",
            Self::IterationLimit => "iteration-budget-exhausted",
            Self::NoDescent => "no-decreasing-step",
            Self::SearchBound => "requested-search-bound-reached",
        }
    }
}
pub struct Fit {
    pub model_to_machine: Pose,
    pub outcome: Outcome,
    pub iterations: usize,
    pub min_scaled_pivot: f64,
    pub observations: Vec<Observation>,
}
fn loss(r: f64, h: f64) -> f64 {
    if r.abs() <= h {
        r * r / 2.
    } else {
        h * (r.abs() - h / 2.)
    }
}
pub fn observe(
    mesh: &Mesh,
    s: &Sample,
    machine_to_model: Pose,
    h: f64,
    radius: f64,
) -> Result<Observation, Error> {
    let center = machine_to_model.point(s.center);
    let near = mesh.nearest(center)?;
    let residual = near.distance - radius;
    let facing = dot(near.normal, mv(machine_to_model.r, s.approach)) < 0.;
    let weight = if residual.abs() <= h {
        1.
    } else {
        h / residual.abs()
    };
    if !residual.is_finite() || !weight.is_finite() {
        return Err(Error::Data(
            "Nonfinite registration residual; check units and placement.".into(),
        ));
    }
    Ok(Observation {
        center,
        near,
        residual,
        weight,
        facing,
    })
}
fn observations(
    mesh: &Mesh,
    samples: &[Sample],
    p: Pose,
    r: &Request,
) -> Result<Vec<Observation>, Error> {
    samples
        .iter()
        .map(|s| observe(mesh, s, p, r.huber, r.radius))
        .collect()
}
fn usable(samples: &[Sample], obs: &[Observation], r: &Request) -> bool {
    samples
        .iter()
        .zip(obs)
        .filter(|(s, _)| s.usage == Use::Fit)
        .all(|(_, o)| o.facing && o.residual.abs() <= r.correspondence)
}
fn objective(samples: &[Sample], obs: &[Observation], r: &Request) -> f64 {
    samples
        .iter()
        .zip(obs)
        .filter(|(s, _)| s.usage == Use::Fit)
        .map(|(_, o)| loss(o.residual, r.huber))
        .sum()
}
fn solve(samples: &[Sample], obs: &[Observation], r: &Request) -> Result<(Vec<f64>, f64), Error> {
    let mut rows = Vec::new();
    let mut rhs = Vec::new();
    for (s, o) in samples.iter().zip(obs).filter(|(s, _)| s.usage == Use::Fit) {
        let n = o.near.normal;
        let rotation = cross(o.center, n);
        let mut row = n.to_vec();
        if r.planar {
            row.push(rotation[2]);
        } else {
            row.extend(rotation);
        }
        let w = o.weight.sqrt();
        rows.push(row.into_iter().map(|x| x * w).collect());
        rhs.push(-o.residual * w);
        if !o.facing || o.residual.abs() > r.correspondence {
            return Err(Error::Data(format!("Fitting contact {}:{} has no facing association inside the requested residual bound. Correct the initial placement or selection; this point was not silently excluded.",s.capture.as_str(),s.sequence)));
        }
    }
    least_squares(&rows, &rhs)
}
pub fn run(mesh: &Mesh, samples: &[Sample], r: &Request) -> Result<Fit, Error> {
    mesh.fitting_geometry()?;
    let mut pose = r.initial.inverse();
    let mut obs = observations(mesh, samples, pose, r)?;
    let mut outcome = Outcome::IterationLimit;
    let mut iterations = 0;
    let mut min_scaled_pivot = 1_f64;
    for step in 0..r.iterations {
        let (delta, pivot) = solve(samples, &obs, r)?;
        min_scaled_pivot = pivot;
        iterations = step + 1;
        let previous = objective(samples, &obs, r);
        let mut fraction = 1.;
        let mut accepted = None;
        let mut shortened_by_bound = false;
        loop {
            let next = pose.then(Pose::increment(&delta, r.planar, fraction));
            let displacement = samples
                .iter()
                .filter(|s| s.usage == Use::Fit)
                .map(|s| norm(sub(next.point(s.center), pose.point(s.center))))
                .fold(0_f64, f64::max);
            let model_pose = next.inverse();
            let relative = r.initial.inverse().then(model_pose);
            let within = norm(sub(model_pose.t, r.initial.t)) <= r.max_translation
                && relative.rotation_angle() <= r.max_rotation;
            shortened_by_bound |= !within;
            let candidate = observations(mesh, samples, next, r)?;
            if within
                && usable(samples, &candidate, r)
                && objective(samples, &candidate, r) <= previous
            {
                accepted = Some((next, candidate, displacement));
                break;
            }
            // Halving is an optimization line search, not a hardware retry count.
            // Numerical stagnation retains the old candidate and its stop reason.
            if displacement <= r.convergence || fraction / 2. == fraction || fraction == 0. {
                outcome = if within {
                    Outcome::NoDescent
                } else {
                    Outcome::SearchBound
                };
                break;
            }
            fraction /= 2.;
        }
        match accepted {
            Some((next, candidate, displacement)) => {
                pose = next;
                obs = candidate;
                if displacement <= r.convergence {
                    outcome = if fraction == 1. {
                        Outcome::Converged
                    } else if shortened_by_bound {
                        Outcome::SearchBound
                    } else {
                        Outcome::NoDescent
                    };
                    break;
                }
            }
            None => break,
        }
    }
    // Rematching changes the Jacobian. Check observability at the final pose too.
    let (_, pivot) = solve(samples, &obs, r)?;
    min_scaled_pivot = min_scaled_pivot.min(pivot);
    let model_to_machine = pose.inverse().validate()?;
    Ok(Fit {
        model_to_machine,
        outcome,
        iterations,
        min_scaled_pivot,
        observations: obs,
    })
}
