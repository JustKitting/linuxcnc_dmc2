use super::super::super::{geometry::*, probe::Sample, Error};
use super::super::{
    surface,
    surface::local::{Checks, Local},
};
use super::{grid::Grid, request::Request};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    Fit,
    Support,
    Band,
    Check,
    CheckConflict,
}
impl Missing {
    pub fn description(self) -> (&'static str, &'static str) {
        match self {
            Self::Fit=>("nearest-local-fit-unresolved","Resolve the nearest retained patch's fit or spatial support. A farther patch was not substituted."),
            Self::Support=>("outside-local-measurement-support","Acquire or select measurements covering this region. No surface was extended across this unsupported location."),
            Self::Band=>("outside-local-normal-band","This point is outside the declared local distance band. Inspect the reconstruction region and measured support before changing that assumption."),
            Self::Check=>("nearest-local-check-missing","Retain independent observations checking this local patch before using it for the reconstructed surface."),
            Self::CheckConflict=>("nearest-local-check-disagreement","Resolve the retained independent check disagreement; the failing contact remains in the source bundle."),
        }
    }
}
#[derive(Clone, Copy)]
pub struct Node {
    pub seed: usize,
    pub value: Result<f64, Missing>,
}
pub struct Field<'a> {
    pub nodes: Vec<Node>,
    pub local: Vec<Local<'a>>,
}
pub fn run<'a>(
    grid: &Grid,
    samples: &[Sample],
    stations: &'a [surface::Station],
    sr: &surface::request::Request,
    r: &Request,
) -> Result<Field<'a>, Error> {
    // Reserve the worst case for field lookup, checks and candidate facets.
    let n = grid
        .count
        .checked_add(samples.len())
        .and_then(|n| n.checked_add(r.triangles));
    let required = n.and_then(|n| n.checked_mul(stations.len()));
    if required.is_none_or(|n| n > r.comparisons) {
        return Err(Error::Input(format!("The grid, independent checks and facet budget require {} local comparisons, above max_field_comparisons={}. Increase the explicit budget or reduce the numerical domain; no region was omitted.",required.map(|n|n.to_string()).unwrap_or_else(||"an unrepresentable number of".into()),r.comparisons)));
    }
    if stations.is_empty() {
        return Err(Error::Data("The source surface has no fitting stations. Select retained fine contacts and calculate a surface analysis first.".into()));
    }
    let local = surface::local::build(samples, stations, sr);
    let lookup = local
        .iter()
        .map(|l| (l.station.seed, l))
        .collect::<BTreeMap<_, _>>();
    let mut nodes = Vec::with_capacity(grid.count);
    for index in 0..grid.count {
        let p = grid.point(index);
        let mut best = (f64::INFINITY, stations[0].seed);
        for s in stations {
            let distance = norm(sub(p, samples[s.seed].center));
            if !distance.is_finite() {
                return Err(Error::Data("Surface lookup distance overflowed. Inspect the source and reconstruction coordinate scales.".into()));
            }
            if distance < best.0 {
                best = (distance, s.seed);
            }
        }
        let value = if let Some(l) = lookup.get(&best.1) {
            let d = dot(sub(p, l.patch.surface), l.patch.normal);
            if !d.is_finite() {
                return Err(Error::Data("Surface signed-distance arithmetic overflowed. Inspect source units and grid bounds.".into()));
            }
            if !l.support.contains(p, l.patch, sr) {
                Err(Missing::Support)
            } else if d.abs() > r.band {
                Err(Missing::Band)
            } else {
                match l.checks {
                    Checks::Missing => Err(Missing::Check),
                    Checks::Disagrees => Err(Missing::CheckConflict),
                    Checks::Within => Ok(d),
                }
            }
        } else {
            Err(Missing::Fit)
        };
        nodes.push(Node {
            seed: best.1,
            value,
        });
    }
    Ok(Field { nodes, local })
}
impl Field<'_> {
    pub fn facet_support(
        &self,
        vertices: &[V; 3],
        sr: &surface::request::Request,
        r: &Request,
    ) -> Option<usize> {
        let normal = cross(sub(vertices[1], vertices[0]), sub(vertices[2], vertices[0]));
        let length = norm(normal);
        if length == 0. || !length.is_finite() {
            return None;
        }
        let normal = normal.map(|v| v / length);
        self.local
            .iter()
            .filter(|l| l.checks == Checks::Within)
            .find(|l| {
                dot(normal, l.patch.normal) > 0.
                    && vertices.iter().all(|v| {
                        dot(sub(*v, l.patch.surface), l.patch.normal).abs()
                            <= r.residual.min(r.band)
                    })
                    && l.support.covers_points(vertices, l.patch, sr)
            })
            .map(|l| l.station.seed)
    }
}
