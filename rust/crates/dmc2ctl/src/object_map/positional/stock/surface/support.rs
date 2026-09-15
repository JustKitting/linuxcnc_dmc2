//! Measured planar support shared by checking and material assessment.
use super::super::super::probe::Sample;
use super::{geometry::*, request::Request, Patch, Station};
type P = [f64; 2];
fn cross2(a: P, b: P, c: P) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
pub struct Support {
    u: V,
    v: V,
    hull: Vec<P>,
    points: Vec<P>,
}
impl Support {
    pub fn new(samples: &[Sample], station: &Station, patch: &Patch, r: &Request) -> Self {
        // This basis parameterizes the measured plane; it does not align the
        // stock to a machine axis or supply a missing surface normal.
        let axis = (0..3)
            .min_by(|a, b| patch.normal[*a].abs().total_cmp(&patch.normal[*b].abs()))
            .unwrap();
        let mut direction = [0.; 3];
        direction[axis] = 1.;
        let u = cross(patch.normal, direction);
        let u = u.map(|x| x / norm(u));
        let v = cross(patch.normal, u);
        let points = station
            .neighbours
            .iter()
            .map(|i| {
                let d = sub(samples[*i].center, patch.center).map(|x| x / r.neighborhood);
                [dot(d, u), dot(d, v)]
            })
            .collect::<Vec<_>>();
        let mut sorted = points.clone();
        sorted.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
        sorted.dedup();
        let mut hull = Vec::new();
        for seq in [sorted.clone(), sorted.into_iter().rev().collect()] {
            let mut half: Vec<P> = Vec::new();
            for p in seq {
                while half.len() >= 2 && cross2(half[half.len() - 2], half[half.len() - 1], p) <= 0.
                {
                    half.pop();
                }
                half.push(p);
            }
            half.pop();
            hull.extend(half);
        }
        Self { u, v, hull, points }
    }
    pub fn contains(&self, p: V, patch: &Patch, r: &Request) -> bool {
        let d = sub(p, patch.center).map(|x| x / r.neighborhood);
        let q = [dot(d, self.u), dot(d, self.v)];
        if self.hull.len() < 3 || !q.iter().all(|x| x.is_finite()) {
            return false;
        }
        for (a, b) in self
            .hull
            .iter()
            .zip(self.hull.iter().cycle().skip(1))
            .take(self.hull.len())
        {
            let first = (b[0] - a[0]) * (q[1] - a[1]);
            let second = (b[1] - a[1]) * (q[0] - a[0]);
            let roundoff = f64::EPSILON * (self.hull.len() as f64) * (first.abs() + second.abs());
            if first - second < -roundoff {
                return false;
            }
        }
        self.points
            .iter()
            .any(|p| (p[0] - q[0]).hypot(p[1] - q[1]) * r.neighborhood <= r.support_gap)
    }
    pub fn covers(&self, p: V, radius: f64, patch: &Patch, r: &Request) -> bool {
        let d = sub(p, patch.surface).map(|x| x / r.neighborhood);
        let q = [dot(d, self.u), dot(d, self.v)];
        let radius = radius / r.neighborhood;
        if self.hull.len() < 3 || !q.iter().all(|x| x.is_finite()) || !radius.is_finite() {
            return false;
        }
        // A disk covering the projected subtriangle must fit wholly inside
        // the support hull and a retained neighbour's support-gap disk.
        for (a, b) in self
            .hull
            .iter()
            .zip(self.hull.iter().cycle().skip(1))
            .take(self.hull.len())
        {
            let edge = (b[0] - a[0]).hypot(b[1] - a[1]);
            if cross2(*a, *b, q) < radius * edge {
                return false;
            }
        }
        self.points
            .iter()
            .any(|p| ((p[0] - q[0]).hypot(p[1] - q[1]) + radius) * r.neighborhood <= r.support_gap)
    }
    pub fn covers_points(&self, points: &[V], patch: &Patch, r: &Request) -> bool {
        if points.is_empty() || points.iter().any(|p| !self.contains(*p, patch, r)) {
            return false;
        }
        let projected = points
            .iter()
            .map(|p| {
                let d = sub(*p, patch.center).map(|x| x / r.neighborhood);
                [dot(d, self.u), dot(d, self.v)]
            })
            .collect::<Vec<_>>();
        // The hull and a single neighbour's support disk are convex. If
        // they contain all facet vertices, they contain its entire projection.
        self.points.iter().any(|p| {
            projected
                .iter()
                .all(|q| (p[0] - q[0]).hypot(p[1] - q[1]) * r.neighborhood <= r.support_gap)
        })
    }
    pub fn json(&self, patch: &Patch, r: &Request) -> String {
        let vertices = self
            .hull
            .iter()
            .map(|p| {
                json(add(
                    patch.surface,
                    add(
                        scale(self.u, p[0] * r.neighborhood),
                        scale(self.v, p[1] * r.neighborhood),
                    ),
                ))
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{{\"projected_neighbour_hull_machine_mm\":[{vertices}],\"max_nearest_sample_distance_mm\":{},\"interpretation\":\"Local planar interpolation assumption inside the projected neighbour hull and within the selected distance of a neighbour; this is not observed material coverage or a closed solid.\"}}",r.support_gap)
    }
}
