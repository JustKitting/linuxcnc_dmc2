//! Signed distances to a simple measured XY polygon; no rectangle or convex hull.
use super::super::super::{geometry::*, Error};
pub struct Polygon {
    pub vertices: Vec<V>,
}
fn cross2(a: V, b: V, c: V) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn on(a: V, b: V, p: V) -> bool {
    cross2(a, b, p) == 0. && (0..2).all(|i| p[i] >= a[i].min(b[i]) && p[i] <= a[i].max(b[i]))
}
fn crossing(a: V, b: V, c: V, d: V) -> bool {
    let [u, v, w, z] = [
        cross2(a, b, c),
        cross2(a, b, d),
        cross2(c, d, a),
        cross2(c, d, b),
    ];
    ((u > 0. && v < 0. || u < 0. && v > 0.) && (w > 0. && z < 0. || w < 0. && z > 0.))
        || on(a, b, c)
        || on(a, b, d)
        || on(c, d, a)
        || on(c, d, b)
}
impl Polygon {
    pub fn new(vertices: Vec<V>) -> Result<Self, Error> {
        if vertices.len() < 3 || vertices.iter().any(|p| !finite(*p)) {
            return Err(Error::Data("A footprint boundary needs at least three finite vertices; resolve the source outline.".into()));
        }
        let origin = vertices[0];
        let extent = vertices
            .iter()
            .map(|p| (p[0] - origin[0]).hypot(p[1] - origin[1]))
            .fold(0_f64, f64::max);
        if extent == 0. || !extent.is_finite() {
            return Err(Error::Data("The source outline has no finite XY extent; inspect its units and retained contacts.".into()));
        }
        let v = vertices
            .iter()
            .map(|p| [(p[0] - origin[0]) / extent, (p[1] - origin[1]) / extent, 0.])
            .collect::<Vec<_>>();
        let count = v.len();
        for i in 0..count {
            let a = v[i];
            let b = v[(i + 1) % count];
            if a == b {
                return Err(Error::Data(format!("Outline stations {i} and {} coincide. Refine the source estimate; no vertex was silently removed.",(i+1)%count)));
            }
            let next = v[(i + 2) % count];
            if on(a, b, next) || on(b, next, a) {
                return Err(Error::Data(format!("Outline edges reverse or overlap at station {}; resolve its retained contacts.",(i+1)%count)));
            }
            for j in i + 1..count {
                if j == i + 1 || i == 0 && j == count - 1 {
                    continue;
                }
                if crossing(a, b, v[j], v[(j + 1) % count]) {
                    return Err(Error::Data(format!("Outline segments {i} and {j} intersect or touch out of sequence. Refine the source estimate before footprint placement; no convex hull was substituted.")));
                }
            }
        }
        Ok(Self { vertices })
    }
    pub fn distance(&self, p: V) -> Result<(f64, usize), Error> {
        if !finite(p) {
            return Err(Error::Data(
                "Footprint query overflowed; inspect placement bounds and STL units.".into(),
            ));
        }
        let mut inside = false;
        let mut best = (f64::INFINITY, 0);
        for (i, a) in self.vertices.iter().enumerate() {
            let b = self.vertices[(i + 1) % self.vertices.len()];
            let d = [b[0] - a[0], b[1] - a[1]];
            let length = d[0].hypot(d[1]);
            let q = [p[0] - a[0], p[1] - a[1]];
            let along = (q[0] * (d[0] / length) + q[1] * (d[1] / length)).clamp(0., length);
            let distance = (q[0] - along * (d[0] / length)).hypot(q[1] - along * (d[1] / length));
            if !distance.is_finite() {
                return Err(Error::Data("Polygon distance overflowed; reduce incompatible coordinate scales in the request.".into()));
            }
            if distance < best.0 {
                best = (distance, i);
            }
            if (a[1] > p[1]) != (b[1] > p[1]) {
                let at = (p[1] - a[1]) / (b[1] - a[1]);
                if p[0] - a[0] < at * (b[0] - a[0]) {
                    inside = !inside;
                }
            }
        }
        Ok((if inside { -best.0 } else { best.0 }, best.1))
    }
}
