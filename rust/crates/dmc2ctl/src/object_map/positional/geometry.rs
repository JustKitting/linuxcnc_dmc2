//! Millimetre geometry, rigid transforms and scaled least squares. No machine IO.
use super::super::Error;
pub type V = [f64; 3];
pub type M = [[f64; 3]; 3];
pub const IDENTITY: M = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
pub fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
pub fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
pub fn scale(a: V, s: f64) -> V {
    a.map(|v| v * s)
}
pub fn dot(a: V, b: V) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
pub fn norm(a: V) -> f64 {
    a[0].hypot(a[1]).hypot(a[2])
}
pub fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn mv(m: M, p: V) -> V {
    m.map(|r| dot(r, p))
}
pub fn transpose(m: M) -> M {
    std::array::from_fn(|i| std::array::from_fn(|j| m[j][i]))
}
pub fn mm(a: M, b: M) -> M {
    let b = transpose(b);
    a.map(|r| b.map(|c| dot(r, c)))
}
pub fn csv(p: V) -> String {
    p.map(|x| x.to_string()).join(",")
}
pub fn json(p: V) -> String {
    format!("[{}]", csv(p))
}
pub fn finite(p: V) -> bool {
    p.iter().all(|x| x.is_finite())
}
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub r: M,
    pub t: V,
}
impl Pose {
    pub fn point(self, p: V) -> V {
        add(mv(self.r, p), self.t)
    }
    pub fn inverse(self) -> Self {
        let r = transpose(self.r);
        Self {
            r,
            t: scale(mv(r, self.t), -1.),
        }
    }
    pub fn then(self, next: Self) -> Self {
        Self {
            r: mm(next.r, self.r),
            t: next.point(self.t),
        }
    }
    pub fn from_euler(deg: V, t: V) -> Self {
        let [x, y, z] = deg.map(f64::to_radians);
        let (sx, cx) = x.sin_cos();
        let (sy, cy) = y.sin_cos();
        let (sz, cz) = z.sin_cos();
        let rx = [[1., 0., 0.], [0., cx, -sx], [0., sx, cx]];
        let ry = [[cy, 0., sy], [0., 1., 0.], [-sy, 0., cy]];
        let rz = [[cz, -sz, 0.], [sz, cz, 0.], [0., 0., 1.]];
        Self {
            r: mm(mm(rz, ry), rx),
            t,
        }
    }
    pub fn increment(delta: &[f64], planar: bool, factor: f64) -> Self {
        let t = std::array::from_fn(|i| delta[i] * factor);
        let w = if planar {
            [0., 0., delta[3] * factor]
        } else {
            [delta[3] * factor, delta[4] * factor, delta[5] * factor]
        };
        let a = norm(w);
        if a == 0. {
            return Self { r: IDENTITY, t };
        }
        let u = scale(w, 1. / a);
        let (s, c) = a.sin_cos();
        let k = [[0., -u[2], u[1]], [u[2], 0., -u[0]], [-u[1], u[0], 0.]];
        Self {
            r: std::array::from_fn(|i| {
                std::array::from_fn(|j| c * IDENTITY[i][j] + (1. - c) * u[i] * u[j] + s * k[i][j])
            }),
            t,
        }
    }
    pub fn rotation_angle(self) -> f64 {
        ((self.r[0][0] + self.r[1][1] + self.r[2][2] - 1.) / 2.)
            .clamp(-1., 1.)
            .acos()
    }
    pub fn validate(self) -> Result<Self, Error> {
        // Roundoff budget for the fixed-size matrix products, not a machining tolerance.
        let allowance = 64. * f64::EPSILON;
        let product = mm(self.r, transpose(self.r));
        if !finite(self.t)
            || !self.r.iter().all(|r| finite(*r))
            || product.iter().enumerate().any(|(i, r)| {
                r.iter()
                    .enumerate()
                    .any(|(j, v)| (v - IDENTITY[i][j]).abs() > allowance)
            })
            || (dot(self.r[0], cross(self.r[1], self.r[2])) - 1.).abs() > allowance
        {
            return Err(Error::Data("Placement is not a finite proper rigid transform; supply a rotation and translation without scale or reflection.".into()));
        }
        Ok(self)
    }
    pub fn json(self) -> String {
        format!(
            "[[{},{}],[{},{}],[{},{}],[0,0,0,1]]",
            csv(self.r[0]),
            self.t[0],
            csv(self.r[1]),
            self.t[1],
            csv(self.r[2]),
            self.t[2]
        )
    }
}

// Modified Gram-Schmidt QR with column pivoting, column scaling and
// reorthogonalization. A singular geometry is reported, never regularized into
// an invented pose. Rank is numerical, not a dimensional-accuracy assertion.
pub fn least_squares(rows: &[Vec<f64>], rhs: &[f64]) -> Result<(Vec<f64>, f64), Error> {
    let n = rows.first().map(Vec::len).unwrap_or(0);
    if n == 0 || rows.len() < n || rows.len() != rhs.len() {
        return Err(Error::Data("Not enough independent fitting observations for the requested placement degrees of freedom.".into()));
    }
    let mut cols = (0..n)
        .map(|j| rows.iter().map(|r| r[j]).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut scales = cols
        .iter()
        .map(|c| c.iter().fold(0_f64, |a, x| a.hypot(*x)))
        .collect::<Vec<_>>();
    if scales.iter().any(|s| *s == 0. || !s.is_finite()) {
        return Err(Error::Data("The selected surfaces leave a placement direction unobserved. Add independently oriented reference surfaces or choose translation-yaw for a seated part.".into()));
    }
    for (c, s) in cols.iter_mut().zip(&scales) {
        for x in c {
            *x /= s;
        }
    }
    let mut order = (0..n).collect::<Vec<_>>();
    let mut r = vec![vec![0.; n]; n];
    let mut y = vec![0.; n];
    let mut ratio = 1_f64;
    let roundoff = f64::EPSILON * (rows.len().max(n) as f64);
    for k in 0..n {
        let lengths = cols
            .iter()
            .map(|c| c.iter().fold(0_f64, |a, x| a.hypot(*x)))
            .collect::<Vec<_>>();
        let p = (k..n)
            .max_by(|a, b| lengths[*a].total_cmp(&lengths[*b]))
            .unwrap();
        cols.swap(k, p);
        order.swap(k, p);
        scales.swap(k, p);
        for row in r.iter_mut().take(k) {
            row.swap(k, p);
        }
        let l = lengths[p];
        if !l.is_finite() || l <= roundoff {
            return Err(Error::Data("Reference geometry is rank deficient: several placements explain these contacts. Add independent faces/features; no unique placement was published.".into()));
        }
        ratio = ratio.min(l);
        r[k][k] = l;
        for x in &mut cols[k] {
            *x /= l;
        }
        y[k] = cols[k].iter().zip(rhs).map(|(a, b)| a * b).sum();
        for j in k + 1..n {
            for _ in 0..2 {
                // Reorthogonalize to suppress cancellation.
                let d = cols[k]
                    .iter()
                    .zip(&cols[j])
                    .map(|(a, b)| a * b)
                    .sum::<f64>();
                r[k][j] += d;
                for i in 0..rows.len() {
                    cols[j][i] -= d * cols[k][i];
                }
            }
        }
    }
    let mut x = vec![0.; n];
    for i in (0..n).rev() {
        x[i] = (y[i] - (i + 1..n).map(|j| r[i][j] * x[j]).sum::<f64>()) / r[i][i];
    }
    let mut result = vec![0.; n];
    for i in 0..n {
        result[order[i]] = x[i] / scales[i];
    }
    if result.iter().any(|v| !v.is_finite()) {
        return Err(Error::Data(
            "Placement solve overflowed; check model units and initial placement.".into(),
        ));
    }
    Ok((result, ratio))
}
