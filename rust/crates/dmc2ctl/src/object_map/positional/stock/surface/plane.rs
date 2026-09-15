//! Weighted orthogonal plane fit in normalized coordinates. No stock-shape prior.
use super::{geometry::*, Reason};
#[derive(Clone, Copy)]
pub struct Plane {
    pub origin: V,
    pub normal: V,
    pub variance: V,
}
impl Plane {
    pub fn residual(self, p: V) -> f64 {
        dot(self.normal, sub(p, self.origin))
    }
    pub fn project(self, p: V) -> V {
        sub(p, scale(self.normal, self.residual(p)))
    }
}
pub fn fit(points: &[V], weights: &[f64], budget: usize) -> Result<Plane, Reason> {
    let sum = weights.iter().sum::<f64>();
    if sum <= 0. || !sum.is_finite() {
        return Err(Reason::Arithmetic);
    }
    let origin = std::array::from_fn(|i| {
        points
            .iter()
            .zip(weights)
            .map(|(p, w)| p[i] * (w / sum))
            .sum()
    });
    let mut a = [[0.; 3]; 3];
    for (p, w) in points.iter().zip(weights) {
        let d = sub(*p, origin);
        for i in 0..3 {
            for j in 0..3 {
                a[i][j] += (w / sum) * d[i] * d[j];
            }
        }
    }
    if !finite(origin) || !a.iter().all(|r| finite(*r)) {
        return Err(Reason::Arithmetic);
    }
    let trace = a[0][0] + a[1][1] + a[2][2];
    let roundoff = f64::EPSILON * (points.len() as f64) * trace;
    let mut vectors = IDENTITY;
    let mut solved = false;
    // Cyclic Jacobi diagonalization. Three pairs are the off-diagonal entries
    // of a symmetric 3D covariance; the user supplies the computational budget.
    for _ in 0..budget {
        let off = a[0][1].abs().max(a[0][2].abs()).max(a[1][2].abs());
        if off <= roundoff {
            solved = true;
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() <= roundoff {
                continue;
            }
            let angle = (2. * a[p][q]).atan2(a[p][p] - a[q][q]) / 2.;
            let (s, c) = angle.sin_cos();
            let (app, apq, aqq) = (a[p][p], a[p][q], a[q][q]);
            a[p][p] = c * c * app + 2. * c * s * apq + s * s * aqq;
            a[q][q] = s * s * app - 2. * c * s * apq + c * c * aqq;
            a[p][q] = 0.;
            a[q][p] = 0.;
            for k in 0..3 {
                if k != p && k != q {
                    let (kp, kq) = (a[k][p], a[k][q]);
                    a[k][p] = c * kp + s * kq;
                    a[p][k] = a[k][p];
                    a[k][q] = -s * kp + c * kq;
                    a[q][k] = a[k][q];
                }
                let (vp, vq) = (vectors[k][p], vectors[k][q]);
                vectors[k][p] = c * vp + s * vq;
                vectors[k][q] = -s * vp + c * vq;
            }
        }
    }
    if !solved && a[0][1].abs().max(a[0][2].abs()).max(a[1][2].abs()) > roundoff {
        return Err(Reason::EigenBudget);
    }
    let mut order = [0, 1, 2];
    order.sort_by(|i, j| a[*i][*i].total_cmp(&a[*j][*j]));
    let variance = order.map(|i| a[i][i]);
    if variance[1] <= roundoff || variance[1] - variance[0] <= roundoff {
        return Err(Reason::UnobservedNormal);
    }
    let n = std::array::from_fn(|i| vectors[i][order[0]]);
    let length = norm(n);
    if length == 0. || !length.is_finite() || !finite(variance) {
        return Err(Reason::Arithmetic);
    }
    Ok(Plane {
        origin,
        normal: n.map(|x| x / length),
        variance,
    })
}
