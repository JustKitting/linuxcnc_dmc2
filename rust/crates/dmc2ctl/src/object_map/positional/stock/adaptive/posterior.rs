//! Laplace posterior for Gaussian contacts and censored free-space observations.
use crate::object_map::Error;
pub struct Censored {
    pub x: Vec<f64>,
    pub lower: f64,
    pub weight: f64,
}
pub fn factor(a: &[Vec<f64>]) -> Result<Vec<Vec<f64>>, Error> {
    let mut l = a.to_vec();
    for i in 0..l.len() {
        for j in 0..=i {
            let v = l[i][j] - (0..j).map(|k| l[i][k] * l[j][k]).sum::<f64>();
            if !v.is_finite() || (i == j && v <= 0.) {
                return Err(Error::Data("Adaptive posterior precision is not finite positive definite. Inspect likelihood scales and basis size; no numerical placement was accepted.".into()));
            }
            l[i][j] = if i == j { v.sqrt() } else { v / l[j][j] };
        }
    }
    Ok(l)
}
pub fn solve(l: &[Vec<f64>], b: &[f64]) -> Result<Vec<f64>, Error> {
    let mut x = vec![0.; b.len()];
    for i in 0..x.len() {
        x[i] = (b[i] - (0..i).map(|j| l[i][j] * x[j]).sum::<f64>()) / l[i][i];
    }
    for i in (0..x.len()).rev() {
        x[i] = (x[i] - (i + 1..x.len()).map(|j| l[j][i] * x[j]).sum::<f64>()) / l[i][i];
    }
    if x.iter().any(|v| !v.is_finite()) {
        return Err(Error::Data("Adaptive posterior solution overflowed. Inspect the likelihood scales and coordinate units.".into()));
    }
    Ok(x)
}
fn energy(a: &[Vec<f64>], b: &[f64], w: &[f64], c: &[Censored]) -> Result<f64, Error> {
    let mut e = 0.;
    for i in 0..w.len() {
        e += 0.5 * a[i][i] * w[i] * w[i] - b[i] * w[i];
        for j in 0..i {
            e += a[i][j] * w[i] * w[j];
        }
    }
    for c in c {
        let v = c.lower - c.x.iter().zip(w).map(|(x, w)| x * w).sum::<f64>();
        // softplus(v) = -log sigmoid(-v), stable on either tail.
        e += c.weight * (v.max(0.) + (-v.abs()).exp().ln_1p());
    }
    if !e.is_finite() {
        return Err(Error::Data("Adaptive censored likelihood overflowed. Inspect noise units and the original empty-space records.".into()));
    }
    Ok(e)
}
fn system(a: &[Vec<f64>], b: &[f64], w: &[f64], c: &[Censored]) -> (Vec<Vec<f64>>, Vec<f64>) {
    let mut h = a.to_vec();
    let mut g = b.to_vec();
    for i in 0..w.len() {
        for j in 0..w.len() {
            g[i] -= a[i.max(j)][i.min(j)] * w[j];
        }
    }
    for c in c {
        let z = c.x.iter().zip(w).map(|(x, w)| x * w).sum::<f64>() - c.lower;
        let p = if z >= 0. {
            1. / (1. + (-z).exp())
        } else {
            z.exp() / (1. + z.exp())
        };
        for i in 0..w.len() {
            g[i] += c.weight * (1. - p) * c.x[i];
            for j in 0..=i {
                h[i][j] += c.weight * p * (1. - p) * c.x[i] * c.x[j];
            }
        }
    }
    (h, g)
}
pub struct Posterior {
    pub lower: Vec<Vec<f64>>,
    pub mean: Vec<f64>,
    pub iterations: usize,
    pub converged: bool,
}
pub fn fit(
    a: &[Vec<f64>],
    b: &[f64],
    c: &[Censored],
    iterations: usize,
    tolerance: f64,
) -> Result<Posterior, Error> {
    let mut mean = solve(&factor(a)?, b)?;
    let mut used = 0;
    let mut converged = c.is_empty();
    while used < iterations && !converged {
        let (h, g) = system(a, b, &mean, c);
        let step = solve(&factor(&h)?, &g)?;
        let old = energy(a, b, &mean, c)?;
        let size = step.iter().fold(0_f64, |m, v| m.max(v.abs()));
        if size <= tolerance {
            converged = true;
            break;
        }
        let mut fraction = 1.;
        let mut accepted = None;
        while fraction > f64::EPSILON {
            let candidate = mean
                .iter()
                .zip(&step)
                .map(|(m, d)| m + fraction * d)
                .collect::<Vec<_>>();
            if candidate == mean {
                break;
            }
            if energy(a, b, &candidate, c)? < old {
                accepted = Some(candidate);
                break;
            }
            fraction *= 0.5;
        }
        used += 1;
        if let Some(candidate) = accepted {
            mean = candidate;
        } else {
            break;
        }
    }
    let (h, g) = system(a, b, &mean, c);
    let lower = factor(&h)?;
    let step = solve(&lower, &g)?;
    converged |= step.iter().all(|s| s.abs() <= tolerance);
    Ok(Posterior {
        lower,
        mean,
        iterations: used,
        converged,
    })
}
