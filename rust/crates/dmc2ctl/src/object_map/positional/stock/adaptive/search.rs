//! Stochastic rigid placement under the current noisy stock model.
use super::super::surface::no_contact::Sweep;
use super::{
    model::Model,
    random::Random,
    request::Request,
    required::{EmptyOverlap, Required},
};
use crate::object_map::{positional::geometry::*, Error};
#[derive(Clone)]
pub struct Region {
    pub point: V,
    pub mean: f64,
    pub sigma: f64,
    pub deficit: f64,
    pub measured_empty: f64,
}
#[derive(Clone)]
pub struct Candidate {
    pub at: [f64; 6],
    pub score: f64,
    pub worst: f64,
    pub regions: Vec<Region>,
    pub empty_overlaps: Vec<EmptyOverlap>,
}
pub struct Search {
    pub best: Candidate,
    pub population: Vec<Candidate>,
    pub history: Vec<(usize, f64, f64)>,
    pub evaluations: usize,
}
pub fn pose(p: [f64; 6]) -> Pose {
    Pose::from_euler(
        [p[3].to_degrees(), p[4].to_degrees(), p[5].to_degrees()],
        [p[0], p[1], p[2]],
    )
}
fn evaluate(
    model: &Model,
    required: &Required,
    empty: &[Sweep],
    r: &Request,
    at: [f64; 6],
) -> Result<Candidate, Error> {
    let pose = pose(at).validate()?;
    let mut regions = Vec::with_capacity(required.samples.len());
    let mut worst = 0_f64;
    let mut sum = 0.;
    let mut weight = 0.;
    for c in &required.samples {
        let point = pose.point(c.point);
        let prediction = model.predict(point)?;
        let sigma = prediction.variance.sqrt();
        let mut measured_empty = 0_f64;
        for sweep in empty {
            // Positive separation deficit is observed air intersecting the
            // required sample itself; it remains independent of the surface prior.
            measured_empty = measured_empty.max(-sweep.ball_separation(point, 0.)?);
        }
        let deficit = (prediction.mean + r.confidence * sigma + c.radius + r.clearance)
            .max(measured_empty)
            .max(0.);
        if !deficit.is_finite() {
            return Err(Error::Data("Adaptive placement loss overflowed. Inspect the retained model scales, source units and search bounds.".into()));
        }
        worst = worst.max(deficit);
        sum += c.weight * deficit * deficit;
        weight += c.weight;
        regions.push(Region {
            point,
            mean: prediction.mean,
            sigma,
            deficit,
            measured_empty,
        });
    }
    let empty_overlaps = required.empty_overlaps(pose, empty, r.clearance)?;
    for overlap in &empty_overlaps {
        worst = worst.max(overlap.deficit);
    }
    let empty_loss = empty_overlaps
        .iter()
        .map(|s| s.deficit * s.deficit)
        .sum::<f64>()
        / empty.len().max(1) as f64;
    let score = (sum / weight + empty_loss).sqrt();
    if !score.is_finite() {
        return Err(Error::Data("Adaptive placement aggregate overflowed. Inspect the model/search scale before retrying.".into()));
    }
    Ok(Candidate {
        at,
        score,
        worst,
        regions,
        empty_overlaps,
    })
}
pub fn run(
    model: &Model,
    required: &Required,
    empty: &[Sweep],
    r: &Request,
    random: &mut Random,
) -> Result<Search, Error> {
    if required.samples.is_empty() {
        return Err(Error::Data("The required geometry has no cover samples. Inspect the original STL and its unit conversion.".into()));
    }
    let mut population = Vec::with_capacity(r.population);
    for i in 0..r.population {
        let p = std::array::from_fn(|j| {
            r.lo[j] + (r.hi[j] - r.lo[j]) * if i == 0 { 0.5 } else { random.unit() }
        });
        population.push(evaluate(model, required, empty, r, p)?);
    }
    let mut count = population.len();
    let mut history = Vec::new();
    let mut best = population
        .iter()
        .min_by(|a, b| a.score.total_cmp(&b.score))
        .unwrap()
        .clone();
    history.push((count, best.score, best.worst));
    while count < r.evaluations && best.worst > 0. {
        for parent in 0..population.len() {
            if count == r.evaluations {
                break;
            }
            let mut donors = Vec::new();
            while donors.len() < 3 {
                let i = random.index(population.len());
                if i != parent && !donors.contains(&i) {
                    donors.push(i);
                }
            }
            let forced = random.index(6);
            let mut at = population[parent].at;
            for j in 0..6 {
                if j == forced || random.unit() < r.crossover {
                    let v = population[donors[0]].at[j]
                        + r.mutation * (population[donors[1]].at[j] - population[donors[2]].at[j]);
                    // Re-sample an out-of-domain trial rather than accumulating
                    // artificial boundary populations by clipping every donor.
                    at[j] = if v >= r.lo[j] && v <= r.hi[j] {
                        v
                    } else {
                        r.lo[j] + random.unit() * (r.hi[j] - r.lo[j])
                    };
                }
            }
            let trial = evaluate(model, required, empty, r, at)?;
            count += 1;
            if trial.score < population[parent].score {
                if trial.score < best.score {
                    best = trial.clone();
                    history.push((count, best.score, best.worst));
                }
                population[parent] = trial;
            }
        }
    }
    Ok(Search {
        best,
        population,
        history,
        evaluations: count,
    })
}
