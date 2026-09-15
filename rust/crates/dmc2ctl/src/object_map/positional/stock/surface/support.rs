//! Measured planar support shared by checking and material assessment.
use super::super::super::{probe::Sample, request::Use, Error};
use super::{geometry::*, request::Request, Patch, Station};
use crate::object_map::record::quote;
type P = [f64; 2];
fn cross2(a: P, b: P, c: P) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
pub struct Support {
    u: V,
    v: V,
    hull: Vec<P>,
    points: Vec<P>,
    no_contact: std::sync::Arc<[super::no_contact::Sweep]>,
    pub conflicts: Vec<Conflict>,
}
pub struct Conflict {
    pub contact: usize,
    pub miss: usize,
    pub surface: V,
}
impl Support {
    pub fn new(
        samples: &[Sample],
        station: &Station,
        patch: &Patch,
        r: &Request,
    ) -> Result<Self, Error> {
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
        let mut result = Self {
            u,
            v,
            hull,
            points,
            no_contact: station.no_contact.clone(),
            conflicts: Vec::new(),
        };
        if !result.no_contact.is_empty() {
            for (contact, s) in samples.iter().enumerate() {
                if !station.neighbours.contains(&contact)
                    && !(s.usage == Use::Check
                        && dot(s.approach, patch.normal) < 0.
                        && result.contains_hull(s.center, patch, r))
                {
                    continue;
                }
                // Retain the original contact's normal offset, not its
                // projection onto the fitted plane: a residual is evidence.
                let surface = sub(s.center, scale(patch.normal, r.probe.radius));
                for (miss, sweep) in result.no_contact.iter().enumerate() {
                    if sweep.excludes_point(surface)? {
                        result.conflicts.push(Conflict {
                            contact,
                            miss,
                            surface,
                        });
                    }
                }
            }
        }
        Ok(result)
    }
    fn surface_projection(&self, p: V, patch: &Patch) -> V {
        let d = sub(p, patch.center);
        add(
            patch.surface,
            add(scale(self.u, dot(d, self.u)), scale(self.v, dot(d, self.v))),
        )
    }
    pub fn excluded(&self, p: V, patch: &Patch) -> Result<bool, Error> {
        let surface = self.surface_projection(p, patch);
        for miss in self.no_contact.iter() {
            if miss.excludes_point(surface)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn contains(&self, p: V, patch: &Patch, r: &Request) -> Result<bool, Error> {
        Ok(self.contains_hull(p, patch, r) && !self.excluded(p, patch)?)
    }
    pub fn contains_hull(&self, p: V, patch: &Patch, r: &Request) -> bool {
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
    pub fn covers(&self, p: V, radius: f64, patch: &Patch, r: &Request) -> Result<bool, Error> {
        let surface = self.surface_projection(p, patch);
        for miss in self.no_contact.iter() {
            if miss.overlaps_ball(surface, radius)? {
                return Ok(false);
            }
        }
        let d = sub(p, patch.surface).map(|x| x / r.neighborhood);
        let q = [dot(d, self.u), dot(d, self.v)];
        let radius = radius / r.neighborhood;
        if self.hull.len() < 3 || !q.iter().all(|x| x.is_finite()) || !radius.is_finite() {
            return Ok(false);
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
                return Ok(false);
            }
        }
        Ok(self
            .points
            .iter()
            .any(|p| ((p[0] - q[0]).hypot(p[1] - q[1]) + radius) * r.neighborhood <= r.support_gap))
    }
    pub fn covers_points(
        &self,
        points: &[V; 3],
        patch: &Patch,
        r: &Request,
    ) -> Result<bool, Error> {
        for p in points {
            if !self.contains(*p, patch, r)? {
                return Ok(false);
            }
        }
        if !self.no_contact.is_empty() {
            let v = points.map(|p| self.surface_projection(p, patch));
            let normal = cross(sub(v[1], v[0]), sub(v[2], v[0]));
            if !norm(normal).is_finite() || norm(normal) == 0. {
                return Ok(false);
            }
            let triangle = crate::object_map::positional::mesh::Triangle { v, n: patch.normal };
            for miss in self.no_contact.iter() {
                if miss.overlaps_triangle(triangle)? {
                    return Ok(false);
                }
            }
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
        Ok(self.points.iter().any(|p| {
            projected
                .iter()
                .all(|q| (p[0] - q[0]).hypot(p[1] - q[1]) * r.neighborhood <= r.support_gap)
        }))
    }
    /// Continuous lower support margin for a covering ball. Each component
    /// is 1-Lipschitz in p; min/max preserve that movement bound. No patch is
    /// silently dropped when a placement leaves its measured support domain.
    pub fn margin(&self, p: V, radius: f64, patch: &Patch, r: &Request) -> Result<f64, Error> {
        if self.hull.len() < 3 || self.points.is_empty() {
            return Err(Error::Data("A retained placement comparison has no planar support hull. Recalculate its source surface and material assessment before refining placement.".into()));
        }
        let d = sub(p, patch.surface).map(|x| x / r.neighborhood);
        let q = [dot(d, self.u), dot(d, self.v)];
        let mut margin = f64::INFINITY;
        if !q.iter().all(|x| x.is_finite()) || !radius.is_finite() {
            return Err(Error::Data("Support coordinates overflowed. Inspect source units and placement bounds before retrying.".into()));
        }
        for (a, b) in self
            .hull
            .iter()
            .zip(self.hull.iter().cycle().skip(1))
            .take(self.hull.len())
        {
            let edge = (b[0] - a[0]).hypot(b[1] - a[1]);
            let slack = cross2(*a, *b, q) / edge * r.neighborhood - radius;
            if !slack.is_finite() || edge == 0. {
                return Err(Error::Data("A retained support edge cannot supply a finite margin. Inspect source geometry and coordinate scale; no edge was omitted.".into()));
            }
            margin = margin.min(slack);
        }
        let mut nearby = f64::NEG_INFINITY;
        for v in &self.points {
            let slack = r.support_gap - (v[0] - q[0]).hypot(v[1] - q[1]) * r.neighborhood - radius;
            if !slack.is_finite() {
                return Err(Error::Data("A retained support-neighbour margin overflowed. Inspect source units and placement bounds; no neighbour was omitted.".into()));
            }
            nearby = nearby.max(slack);
        }
        margin = margin.min(nearby);
        let projection = self.surface_projection(p, patch);
        for sweep in self.no_contact.iter() {
            margin = margin.min(sweep.ball_separation(projection, radius)?);
        }
        if !margin.is_finite() || !finite(p) {
            return Err(Error::Data("Measured support margin overflowed. Inspect source units, cover radius and placement bounds before retrying.".into()));
        }
        Ok(margin)
    }
    pub fn conflict_json(
        &self,
        c: &Conflict,
        samples: &[Sample],
        station: &Station,
        patch: &Patch,
    ) -> String {
        let s = &samples[c.contact];
        let seed = &samples[station.seed];
        let (kind, message) = super::no_contact::Issue::Conflict.description();
        format!("{{\"kind\":{},\"patch_source\":{{\"capture\":{},\"sequence\":{}}},\"outward_normal\":{},\"contact\":{{\"capture\":{},\"sequence\":{},\"use\":{}}},\"no_contact_source\":{},\"contact_surface_machine_mm\":{},\"message\":{},\"machine_action_authorized\":false}}",quote(kind),quote(seed.capture.as_str()),seed.sequence,json(patch.normal),quote(s.capture.as_str()),s.sequence,quote(s.usage.name()),self.no_contact[c.miss].reference(),json(c.surface),quote(message))
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
        let constraints = if matches!(r.no_contact, super::request::NoContactModel::Legacy) {
            String::new()
        } else {
            format!(",\"excluded_no_contact_sources\":[{}],\"exclusion_interpretation\":\"The planar hull is clipped by finite eroded probe sweeps. Facet vertices alone cannot authorize bridging a sweep. The declared no-contact error model is an assumption, not empty-volume certification.\"",self.no_contact.iter().map(|s|s.reference()).collect::<Vec<_>>().join(","))
        };
        format!("{{\"projected_neighbour_hull_machine_mm\":[{vertices}],\"max_nearest_sample_distance_mm\":{},\"interpretation\":\"Local planar interpolation assumption inside the projected neighbour hull and within the selected distance of a neighbour; this is not observed material coverage or a closed solid.\"{constraints}}}",r.support_gap)
    }
}
