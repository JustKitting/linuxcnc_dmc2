//! Unchanged required volume, stochastic interior queries and retained air.
use super::{random::Random, request::Request, surface::no_contact::Sweep};
use crate::object_map::{
    positional::{
        cover,
        geometry::*,
        mesh::{solid::Solid, Mesh},
    },
    Error,
};

pub struct Sample {
    pub point: V,
    pub radius: f64,
    pub weight: f64,
    pub triangle: Option<usize>,
}
pub struct Required<'a> {
    mesh: &'a Mesh,
    solid: Solid<'a>,
    pub samples: Vec<Sample>,
    pub interior_candidates: usize,
}
#[derive(Clone)]
pub struct EmptyOverlap {
    pub source: usize,
    pub deficit: f64,
    pub witness: V,
}
impl<'a> Required<'a> {
    pub fn build(
        mesh: &'a Mesh,
        covers: &[cover::Sample],
        r: &Request,
        random: &mut Random,
    ) -> Result<Self, Error> {
        let n = mesh.triangles().len();
        // A binary hierarchy has 2*n-1 nodes. Visiting every node for every
        // original triangle bounds validation without an arbitrary geometry cap.
        let visits = n.checked_mul(2).and_then(|v|v.checked_sub(1)).and_then(|v|v.checked_mul(n))
            .ok_or_else(||Error::Input("Required-volume topology size overflows. Inspect the original mesh size before fitting.".into()))?;
        let winding = n.checked_mul(n).ok_or_else(||Error::Input("Required-volume winding size overflows. Inspect the original mesh size before fitting.".into()))?;
        let solid = Solid::required(mesh, visits, winding)?;
        let areas = covers
            .iter()
            .map(|c| {
                norm(cross(
                    sub(c.vertices[1], c.vertices[0]),
                    sub(c.vertices[2], c.vertices[0]),
                )) * 0.5
            })
            .collect::<Vec<_>>();
        let total = areas.iter().sum::<f64>();
        if !total.is_finite() || total <= 0. {
            return Err(Error::Data("Required surface area is not finite and positive. Inspect the unchanged STL units and faces.".into()));
        }
        // Surface and interior are separate probability measures with equal
        // aggregate weight; triangle density and rejection rate cannot reweight them.
        let mut samples = covers
            .iter()
            .zip(areas)
            .map(|(c, a)| Sample {
                point: c.center,
                radius: c.radius,
                weight: a / total,
                triangle: Some(c.triangle),
            })
            .collect::<Vec<_>>();
        let min: V = std::array::from_fn(|i| {
            mesh.triangles()
                .iter()
                .flat_map(|t| t.v.iter().map(|p| p[i]))
                .fold(f64::INFINITY, f64::min)
        });
        let max: V = std::array::from_fn(|i| {
            mesh.triangles()
                .iter()
                .flat_map(|t| t.v.iter().map(|p| p[i]))
                .fold(f64::NEG_INFINITY, f64::max)
        });
        let mut accepted = 0;
        let mut attempted = 0;
        while accepted < r.interior_samples && attempted < r.interior_candidates {
            attempted += 1;
            let p = std::array::from_fn(|i| min[i] + random.unit() * (max[i] - min[i]));
            if solid.distance(p)?.inward > 0. {
                samples.push(Sample {
                    point: p,
                    radius: 0.,
                    weight: 1. / r.interior_samples as f64,
                    triangle: None,
                });
                accepted += 1;
            }
        }
        if accepted != r.interior_samples {
            return Err(Error::Input(format!("Required-volume sampling found {accepted} of {} requested interior points within {} candidates. Increase interior_candidates for this geometry; no missing interior sample was silently accepted.",r.interior_samples,r.interior_candidates)));
        }
        Ok(Self {
            mesh,
            solid,
            samples,
            interior_candidates: attempted,
        })
    }
    pub fn empty_overlaps(
        &self,
        pose: Pose,
        sweeps: &[Sweep],
        clearance: f64,
    ) -> Result<Vec<EmptyOverlap>, Error> {
        let inverse = pose.inverse();
        let mut result = Vec::new();
        for (source, sweep) in sweeps.iter().enumerate() {
            let mut local = sweep.clone();
            local.center_from = inverse.point(sweep.center_from);
            local.center_end = inverse.point(sweep.center_end);
            let mut gap = f64::INFINITY;
            for t in self.mesh.triangles() {
                gap = gap.min(local.triangle_distance(*t)?);
            }
            // If a connected path does not cross the required boundary, its
            // retained start witnesses inside/outside, including enclosed air.
            let distance = self.solid.distance(local.center_from)?;
            let deficit = if distance.inward > 0. {
                distance.inward + sweep.radius + clearance
            } else {
                (sweep.radius + clearance - gap).max(0.)
            };
            if !deficit.is_finite() {
                return Err(Error::Data("Required-volume empty-space separation overflowed. Inspect the original sweep and candidate frame before continuing.".into()));
            }
            if deficit > 0. {
                result.push(EmptyOverlap {
                    source,
                    deficit,
                    witness: sweep.center_from,
                });
            }
        }
        Ok(result)
    }
}
