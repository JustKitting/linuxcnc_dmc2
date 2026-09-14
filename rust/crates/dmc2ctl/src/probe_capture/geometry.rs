//! Geometry from retained grid observations. No machine or HAL access.
use std::collections::{BTreeMap, BTreeSet};

pub(super) type Cell = (i32, i32); // column, row, relative to the first contact
pub(super) type Point = [f64; 2];

pub(super) fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

#[derive(Default)]
pub(super) struct Frontier {
    pending: BTreeSet<(i64, i32, i32)>,
}

impl Frontier {
    fn key((x, y): Cell) -> (i64, i32, i32) {
        (i64::from(x).pow(2) + i64::from(y).pow(2), y, x)
    }

    pub fn observe(&mut self, cell: Cell, hit: bool, observations: &BTreeMap<Cell, bool>) {
        self.pending.remove(&Self::key(cell));
        if hit {
            // Eight neighbours keep diagonal connectivity for a rotated block.
            // An empty frontier means all neighbouring cells were measured.
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let next = (cell.0 + dx, cell.1 + dy);
                    if !observations.contains_key(&next) {
                        self.pending.insert(Self::key(next));
                    }
                }
            }
        }
    }

    pub fn next(&self) -> Option<Cell> {
        self.pending.first().map(|&(_, y, x)| (x, y))
    }
}

fn cross(o: Point, a: Point, b: Point) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

fn hull(points: &[Point]) -> Vec<Point> {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    sorted.dedup();
    let mut lower = Vec::new();
    for &p in &sorted {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper = Vec::new();
    for &p in sorted.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Rectangle {
    pub u: Point,
    pub v: Point,
    pub min: Point,
    pub max: Point,
}

impl Rectangle {
    fn at_angle(points: &[Point], angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        let u = [cos, sin];
        let v = [-sin, cos];
        let mut result = Self {
            u,
            v,
            min: [f64::INFINITY; 2],
            max: [f64::NEG_INFINITY; 2],
        };
        for &p in points {
            for (axis, vector) in [u, v].into_iter().enumerate() {
                let s = dot(p, vector);
                result.min[axis] = result.min[axis].min(s);
                result.max[axis] = result.max[axis].max(s);
            }
        }
        result
    }

    fn area(self) -> f64 {
        (self.max[0] - self.min[0]) * (self.max[1] - self.min[1])
    }

    pub fn fit(hits: &[Point], misses: &[Point], spacing: f64) -> Result<Self, String> {
        let boundary = hull(hits);
        if boundary.len() < 3 {
            return Err("The top contacts do not enclose an area. The cuboid footprint cannot be determined from this grid.".into());
        }
        // A minimum-area enclosing rectangle has an edge parallel to a hull
        // edge. This searches those data-derived angles, not assumed alignment.
        let rectangle = boundary
            .iter()
            .zip(boundary.iter().cycle().skip(1))
            .map(|(a, b)| {
                Self::at_angle(
                    hits,
                    (b[1] - a[1])
                        .atan2(b[0] - a[0])
                        .rem_euclid(std::f64::consts::FRAC_PI_2),
                )
            })
            .min_by(|a, b| a.area().total_cmp(&b.area()))
            .unwrap();
        // The footprint is a contact envelope, including ball-edge contacts.
        // A miss more than one grid-cell diagonal inside that envelope is
        // inconsistent with a single solid cuboid. Do not fill holes silently.
        let uncertainty = std::f64::consts::SQRT_2 * spacing;
        for &p in misses {
            let q = [dot(p, rectangle.u), dot(p, rectangle.v)];
            if (0..2).all(|i| {
                q[i] > rectangle.min[i] + uncertainty && q[i] < rectangle.max[i] - uncertainty
            }) {
                return Err("A measured miss lies inside the fitted block footprint. The retained hit/miss map is inconsistent with a solid cuboid; no side descent was planned.".into());
            }
        }
        Ok(rectangle)
    }

    pub fn stations(
        self,
        spacing: f64,
        radius: f64,
        clearance: f64,
    ) -> Result<Vec<Station>, String> {
        let uncertainty = std::f64::consts::SQRT_2 * spacing;
        // Exclude corners by ball radius + a grid diagonal. The rectangle is
        // provisional; the subsequent side contacts are the actual dimensions.
        let inset = radius + uncertainty;
        let mut result = Vec::new();
        // Adjacent faces around the perimeter, preserving circuit direction.
        for edge in [0, 3, 1, 2] {
            let axis = edge / 2;
            let tangent_axis = 1 - axis;
            let sign = if edge % 2 == 0 { -1.0 } else { 1.0 };
            let base_normal = [self.u, self.v][axis];
            let normal = base_normal.map(|n| n * sign);
            let tangent = [self.u, self.v][tangent_axis];
            let support = if sign < 0.0 {
                self.min[axis]
            } else {
                self.max[axis]
            };
            let begin = self.min[tangent_axis] + inset;
            let end = self.max[tangent_axis] - inset;
            if end - begin < spacing {
                return Err("The measured footprint is too narrow for separate planar-side stations at this spacing and ball radius. No side descent was planned.".into());
            }
            let count = ((end - begin) / spacing).floor() as usize + 1;
            for index in 0..count {
                let index = if matches!(edge, 1 | 2) {
                    count - 1 - index
                } else {
                    index
                };
                let along = begin + index as f64 * spacing;
                let point = [0, 1].map(|i| base_normal[i] * support + tangent[i] * along);
                result.push(Station {
                    edge,
                    normal,
                    approach: [0, 1].map(|i| point[i] + normal[i] * (inset + clearance)),
                    target: [0, 1].map(|i| point[i] - normal[i] * inset),
                });
            }
        }
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Station {
    pub edge: usize,
    pub normal: Point,
    pub approach: Point,
    pub target: Point,
}

#[derive(Debug)]
pub(super) struct Plane {
    pub origin: [f64; 3],
    pub slopes: Point,
    pub rms: f64,
    pub count: usize,
}

/// Fit w = mean_w + a*(u-mean_u) + b*(v-mean_v). No nominal dimensions.
pub(super) fn plane(points: &[[f64; 3]]) -> Option<Plane> {
    if points.len() < 3 {
        return None;
    }
    let origin = [0, 1, 2].map(|i| points.iter().map(|p| p[i]).sum::<f64>() / points.len() as f64);
    let mut uu = 0.0;
    let mut uv = 0.0;
    let mut vv = 0.0;
    let mut uw = 0.0;
    let mut vw = 0.0;
    for p in points {
        let [u, v, w] = [0, 1, 2].map(|i| p[i] - origin[i]);
        uu += u * u;
        uv += u * v;
        vv += v * v;
        uw += u * w;
        vw += v * w;
    }
    let determinant = uu * vv - uv * uv;
    // Relative f64 singularity guard, not an invented measurement tolerance.
    if determinant <= f64::EPSILON * (uu * vv).abs() {
        return None;
    }
    let slopes = [
        (uw * vv - vw * uv) / determinant,
        (vw * uu - uw * uv) / determinant,
    ];
    let rms = (points
        .iter()
        .map(|p| {
            (p[2] - origin[2] - slopes[0] * (p[0] - origin[0]) - slopes[1] * (p[1] - origin[1]))
                .powi(2)
        })
        .sum::<f64>()
        / points.len() as f64)
        .sqrt();
    Some(Plane {
        origin,
        slopes,
        rms,
        count: points.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotated_rectangle_and_complete_miss_boundary() {
        // Offline geometry fixture, not evidence about the physical machine.
        let angle: f64 = 0.37;
        let u = [angle.cos(), angle.sin()];
        let v = [-angle.sin(), angle.cos()];
        let mut observed = BTreeMap::from([((0, 0), true)]);
        let mut frontier = Frontier::default();
        frontier.observe((0, 0), true, &observed);
        while let Some(cell) = frontier.next() {
            let p = [cell.0 as f64, cell.1 as f64];
            observed.insert(cell, dot(p, u).abs() <= 12.0 && dot(p, v).abs() <= 5.0);
            frontier.observe(cell, observed[&cell], &observed);
            assert!(observed.len() < 1000);
        }
        let (hits, misses): (Vec<_>, Vec<_>) = observed.iter().partition(|(_, hit)| **hit);
        let points = |items: Vec<(&Cell, &bool)>| {
            items
                .into_iter()
                .map(|(&(x, y), _)| [x as f64, y as f64])
                .collect::<Vec<_>>()
        };
        let r = Rectangle::fit(&points(hits), &points(misses), 1.0).unwrap();
        assert!((r.u[1].atan2(r.u[0]) - angle).abs() < 0.04);
        let stations = r.stations(1.0, 1.0, 2.0).unwrap();
        for pair in stations.windows(2).filter(|p| p[0].edge == p[1].edge) {
            assert!(
                ((pair[0].approach[0] - pair[1].approach[0])
                    .hypot(pair[0].approach[1] - pair[1].approach[1])
                    - 1.0)
                    .abs()
                    < 1e-12
            );
        }
    }

    #[test]
    fn rejects_an_interior_missing_region_and_singular_plane() {
        let corners = [[-10.0, -5.0], [10.0, -5.0], [10.0, 5.0], [-10.0, 5.0]];
        assert!(Rectangle::fit(&corners, &[[0.0, 0.0]], 1.0).is_err());
        assert!(plane(&[[1.0; 3]; 3]).is_none());
        let fitted = plane(&[
            [0.0, 0.0, 4.0],
            [1.0, 0.0, 6.0],
            [0.0, 1.0, 7.0],
            [1.0, 1.0, 9.0],
        ])
        .unwrap();
        assert_eq!(fitted.slopes, [2.0, 3.0]);
        assert_eq!(fitted.rms, 0.0);
    }
}
