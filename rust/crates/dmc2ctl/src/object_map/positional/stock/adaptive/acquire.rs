//! Placement-directed information acquisition; top and side rays at new heights.
use super::{model::Model, random::Random, request::Request, search::Search};
use crate::{
    object_map::{positional::geometry::*, Error},
    probe_data::{
        mapper_schema::Phase,
        mapper_settings::{Request as Ray, Settings},
    },
};
pub struct Observation {
    pub request: Ray,
    pub predicted_center: V,
    pub gain: f64,
    pub target: V,
    pub bracketed: bool,
}
pub struct Selection {
    pub rows: Vec<Observation>,
    pub considered: usize,
    pub outside: usize,
}
fn center(work: V, offset: V, mount: V) -> V {
    add(add(work, offset), mount)
}
pub fn run(
    model: &Model,
    search: &Search,
    s: &Settings,
    mount: V,
    r: &Request,
    random: &mut Random,
) -> Result<Selection, Error> {
    // Competing placements each contribute their own most uncertain/violated
    // required region. No fixed set of initially supported patches survives.
    let targets = search
        .population
        .iter()
        .filter_map(|p| {
            p.regions
                .iter()
                .max_by(|a, b| (a.deficit + a.sigma).total_cmp(&(b.deficit + b.sigma)))
        })
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Err(Error::Data("Adaptive acquisition has no required-material target. Inspect the design cover and candidate population.".into()));
    }
    let mut rows = Vec::new();
    let mut outside = 0;
    for _ in 0..r.observation_candidates {
        let critical = targets[random.index(targets.len())];
        let anchor = sub(sub(critical.point, mount), s.offset);
        // Sample neighbouring locations as well as the requested material.
        // This is a modelling length, not a fixed trace increment or a move.
        let p = std::array::from_fn(|i| anchor[i] + r.length * random.normal());
        if (0..2).any(|i| p[i] < s.min[i] || p[i] > s.max[i])
            || p[2] < s.floor
            || p[2] >= s.origin[2]
        {
            outside += 1;
            continue;
        }
        let choice = random.index(5);
        let (axis, sign) = match choice {
            0 => (2, -1.),
            1 => (0, 1.),
            2 => (0, -1.),
            3 => (1, 1.),
            _ => (1, -1.),
        };
        let mut from = p;
        from[axis] = if axis == 2 {
            s.origin[2]
        } else if sign > 0. {
            s.min[axis]
        } else {
            s.max[axis]
        };
        if (p[axis] - from[axis]) * sign <= 0. {
            outside += 1;
            continue;
        }
        let q = Ray {
            phase: if axis == 2 { Phase::Grid } else { Phase::Rim },
            edge: if axis == 2 { -1 } else { (choice - 1) as i32 },
            approach: [from[0], from[1]],
            target: p,
        };
        // These are proposed new rays, not a repeat of old acquisition data.
        for point in [from, p, [from[0], from[1], s.origin[2]]] {
            s.bounds(point).map_err(Error::Input)?;
        }
        let mut a = center(from, s.offset, mount);
        let mut b = center(p, s.offset, mount);
        let fa = model.predict(a)?.mean - s.radius;
        let fb = model.predict(b)?.mean - s.radius;
        let bracketed = fa >= 0. && fb <= 0.;
        if bracketed {
            // Refine the predicted first surface bracket. Actual motion still
            // probes the retained whole ray and records its actual trigger.
            while (b[axis] - a[axis]).abs() > r.ray_resolution {
                let m = add(a, scale(sub(b, a), 0.5));
                if m == a || m == b {
                    break;
                }
                if model.predict(m)?.mean - s.radius > 0. {
                    a = m;
                } else {
                    b = m;
                }
            }
        }
        let predicted = if bracketed {
            add(a, scale(sub(b, a), 0.5))
        } else {
            b
        };
        let predicted_value = model.predict(predicted)?;
        let contact_likelihood = (-0.5 * (predicted_value.mean - s.radius).powi(2)
            / (predicted_value.variance + r.contact_sigma * r.contact_sigma))
            .exp();
        let mut gain = 0.;
        for target in &targets {
            let uncertainty = target.sigma * target.sigma;
            let relevance = uncertainty
                / (uncertainty
                    + (target.mean + r.clearance).powi(2)
                    + r.contact_sigma * r.contact_sigma);
            gain += relevance * model.information(target.point, predicted, r.contact_sigma)?;
        }
        gain *= contact_likelihood;
        if !gain.is_finite() {
            return Err(Error::Data("Adaptive measurement ranking overflowed. Inspect the retained posterior and noise scales before exporting.".into()));
        }
        rows.push(Observation {
            request: q,
            predicted_center: predicted,
            gain,
            target: critical.point,
            bracketed,
        });
    }
    rows.sort_by(|a, b| b.gain.total_cmp(&a.gain));
    // Prefer informative, distinct positions in a batch. This changes only
    // numerical selection: execution uses the selected order without retries.
    let mut selected: Vec<Observation> = Vec::new();
    for row in rows {
        if selected
            .iter()
            .any(|p| norm(sub(p.predicted_center, row.predicted_center)) < r.ray_resolution)
        {
            continue;
        }
        selected.push(row);
        if selected.len() == r.observations {
            break;
        }
    }
    Ok(Selection {
        rows: selected,
        considered: r.observation_candidates,
        outside,
    })
}
