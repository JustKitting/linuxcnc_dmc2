//! Explicit enclosed models for embedded, oriented material boundaries.
//! This checks geometry; it does not establish the physical absence of cavities.
use super::super::geometry::*;
use super::{data, intersection, Error, Mesh, V};
use std::collections::{BTreeMap, BTreeSet};

type Key = [u64; 3];
fn key(p: V) -> Key {
    p.map(|x| if x == 0. { 0 } else { x.to_bits() })
}
pub struct Solid<'a> {
    mesh: &'a Mesh,
    model: Model,
    pub topology_visits: usize,
    pub triangle_pairs: usize,
    pub vertices: usize,
    pub shells: Vec<Shell>,
    pub structure_winding_terms: usize,
}
#[derive(Clone, Copy)]
enum Model {
    Stock,
    Required { winding_terms: usize },
}
impl Model {
    fn name(self) -> &'static str {
        match self {
            Self::Stock => "stock",
            Self::Required { .. } => "required-material",
        }
    }
    fn recovery(self) -> &'static str {
        match self {
            Self::Stock => "Inspect reconstruction measurement needs and resolve the source boundary; no holes were capped or unknown volume filled.",
            Self::Required { .. } => "Inspect the unchanged required-operation STL and its oriented material/cavity boundary; attach a corrected revision rather than filling holes or deleting bodies.",
        }
    }
    fn boundary(self) -> &'static str {
        match self {
            Self::Stock => "a closed outward shell",
            Self::Required { .. } => "a closed oriented material boundary",
        }
    }
}
#[derive(Clone)]
pub struct Shell {
    pub first_triangle: usize,
    pub triangle_count: usize,
    pub signed_volume: f64,
    pub surrounding_winding: Option<f64>,
}
pub struct Distance {
    /// Positive inward distance, negative outside, zero on the boundary.
    pub inward: f64,
    pub triangle: usize,
    pub winding: Option<f64>,
}
impl<'a> Solid<'a> {
    pub fn new(mesh: &'a Mesh, max_visits: usize) -> Result<Self, Error> {
        Self::build(mesh, max_visits, Model::Stock)
    }
    pub fn required(
        mesh: &'a Mesh,
        max_visits: usize,
        winding_terms: usize,
    ) -> Result<Self, Error> {
        Self::build(mesh, max_visits, Model::Required { winding_terms })
    }
    fn build(mesh: &'a Mesh, max_visits: usize, model: Model) -> Result<Self, Error> {
        if mesh.boundary_edges != 0 || mesh.inconsistent_edges != 0 || mesh.signed_volume <= 0. {
            return Err(data(format!(
                "The enclosed-{} model needs {}; this mesh has {} open edges, {} inconsistent edges and signed volume {} mm3. {}",
                model.name(), model.boundary(), mesh.boundary_edges, mesh.inconsistent_edges, mesh.signed_volume, model.recovery()
            )));
        }
        let mut vertices: BTreeMap<Key, Vec<[Key; 2]>> = BTreeMap::new();
        let mut edges: BTreeMap<(Key, Key), Vec<usize>> = BTreeMap::new();
        for (i, t) in mesh.triangles.iter().enumerate() {
            let v = t.v.map(key);
            for j in 0..3 {
                vertices
                    .entry(v[j])
                    .or_default()
                    .push([v[(j + 1) % 3], v[(j + 2) % 3]]);
                let (a, b) = (v[j], v[(j + 1) % 3]);
                edges.entry((a.min(b), a.max(b))).or_default().push(i);
            }
        }
        for (vertex, links) in &vertices {
            let mut adjacent: BTreeMap<Key, Vec<Key>> = BTreeMap::new();
            for [a, b] in links {
                adjacent.entry(*a).or_default().push(*b);
                adjacent.entry(*b).or_default().push(*a);
            }
            let mut seen = BTreeSet::new();
            let mut pending = vec![*adjacent.keys().next().unwrap()];
            while let Some(v) = pending.pop() {
                if seen.insert(v) {
                    pending.extend(&adjacent[&v]);
                }
            }
            if adjacent.values().any(|v| v.len() != 2) || seen.len() != adjacent.len() {
                return Err(data(format!(
                    "The {} boundary pinches or branches at vertex {}. Resolve the source geometry and reconstruct a new mesh; an enclosed solid was not accepted.",
                    model.name(), csv(vertex.map(f64::from_bits))
                )));
            }
        }
        let mut adjacency = vec![Vec::new(); mesh.triangles.len()];
        for faces in edges.values() {
            adjacency[faces[0]].push(faces[1]);
            adjacency[faces[1]].push(faces[0]);
        }
        let mut seen = BTreeSet::new();
        let mut components = Vec::new();
        let mut shell_for_triangle = vec![0; mesh.triangles.len()];
        for first in 0..mesh.triangles.len() {
            if seen.contains(&first) {
                continue;
            }
            let mut members = Vec::new();
            let mut pending = vec![first];
            while let Some(i) = pending.pop() {
                if seen.insert(i) {
                    members.push(i);
                    shell_for_triangle[i] = components.len();
                    pending.extend(&adjacency[i]);
                }
            }
            members.sort_unstable();
            components.push(members);
        }
        if matches!(model, Model::Stock) && components.len() != 1 {
            return Err(data(
                "The stock mesh contains multiple disconnected shells. Their material/cavity relationship is not defined by single-shell-enclosed-solid. Resolve that stock model explicitly; no shell was discarded or filled.",
            ));
        }
        let mut triangle_pairs = 0;
        let topology_visits = mesh.spatial.overlap_pairs(&mesh.triangles, max_visits, |a,b| {
            triangle_pairs += 1;
            if intersection::beyond_shared_simplex(&mesh.triangles[a], &mesh.triangles[b])? {
                return Err(data(match model {
                    Model::Stock => format!("Stock triangles {a} and {b} intersect beyond a shared edge or vertex. Inspect their retained reconstruction sources and resolve the geometry; no intersection repair was applied."),
                    Model::Required { .. } => format!("Required-material triangles {a} and {b} intersect beyond a shared edge or vertex. Inspect the original operation geometry and retain a corrected revision; no repair or boolean union was applied."),
                }));
            }
            Ok(())
        })?;
        let mut shells = Vec::new();
        let mut structure_winding_terms = 0;
        if let Model::Required { winding_terms } = model {
            structure_winding_terms = mesh.triangles.len().checked_mul(components.len() - 1)
                .ok_or_else(|| data("Required boundary nesting exceeds the arithmetic computation budget. Inspect component/triangle counts before retrying."))?;
            if structure_winding_terms > winding_terms {
                return Err(data(format!("Required boundary nesting needs {structure_winding_terms} solid-angle terms, exceeding max_winding_terms={winding_terms}. Increase that explicit computation budget; no shell relationship was skipped.")));
            }
            for (shell, members) in components.iter().enumerate() {
                let first_triangle = members[0];
                let origin = mesh.triangles[first_triangle].v[0];
                let signed_volume = members
                    .iter()
                    .map(|&i| {
                        let v = mesh.triangles[i].v.map(|p| sub(p, origin));
                        dot(v[0], cross(v[1], v[2])) / 6.
                    })
                    .sum::<f64>();
                if !signed_volume.is_finite() || signed_volume == 0. {
                    return Err(data(format!("Required shell starting at triangle {first_triangle} has indeterminate signed volume. Inspect its units and geometry; no material orientation was inferred.")));
                }
                // Other embedded shells cannot cross this connected shell.
                // Its original vertex therefore witnesses their nesting without
                // an invented offset from its own boundary.
                let surrounding = winding(
                    mesh.triangles
                        .iter()
                        .enumerate()
                        .filter_map(|(i, t)| (shell_for_triangle[i] != shell).then_some(t)),
                    origin,
                );
                let rounded = surrounding.round();
                let expected = if signed_volume > 0. { 0. } else { 1. };
                if !surrounding.is_finite()
                    || (surrounding - rounded).abs() == 0.5
                    || rounded != expected
                {
                    return Err(data(format!("Required shell starting at triangle {first_triangle} has signed volume {signed_volume} mm3 and surrounding winding {surrounding}; an oriented material boundary requires surrounding winding {expected} for this orientation. Resolve nested material/cavity roles in the original geometry; no body was discarded or filled.")));
                }
                shells.push(Shell {
                    first_triangle,
                    triangle_count: members.len(),
                    signed_volume,
                    surrounding_winding: Some(surrounding),
                });
            }
        } else {
            shells.push(Shell {
                first_triangle: 0,
                triangle_count: mesh.triangles.len(),
                signed_volume: mesh.signed_volume,
                surrounding_winding: None,
            });
        }
        Ok(Self {
            mesh,
            model,
            topology_visits,
            triangle_pairs,
            vertices: vertices.len(),
            shells,
            structure_winding_terms,
        })
    }
    pub fn triangle_count(&self) -> usize {
        self.mesh.triangles.len()
    }
    pub fn distance(&self, p: V) -> Result<Distance, Error> {
        let nearest = self.mesh.nearest(p)?;
        let distance = nearest.distance.abs();
        if distance == 0. {
            return Ok(Distance {
                inward: 0.,
                triangle: nearest.triangle,
                winding: None,
            });
        }
        // Sum oriented solid angles with compensation. Closed embedded outward
        // shells have winding 1 inside, 0 outside. Half separates those values;
        // it is not an error allowance for open/self-intersecting geometry.
        let winding = winding(self.mesh.triangles.iter(), p);
        if !winding.is_finite() || winding == 0.5 {
            return Err(data(format!(
                "The enclosed-{} query has an indeterminate winding value. Preserve this analysis input and inspect the {} geometry/coordinate scale; no inside/outside result was substituted.", self.model.name(), self.model.name()
            )));
        }
        Ok(Distance {
            inward: if winding > 0.5 { distance } else { -distance },
            triangle: nearest.triangle,
            winding: Some(winding),
        })
    }
}
fn winding<'a>(triangles: impl Iterator<Item = &'a super::Triangle>, p: V) -> f64 {
    let (mut sum, mut compensation) = (0., 0.);
    for t in triangles {
        let [a, b, c] = t.v.map(|v| {
            let d = sub(v, p);
            let n = norm(d);
            d.map(|x| x / n)
        });
        let angle = 2. * dot(a, cross(b, c)).atan2(1. + dot(a, b) + dot(b, c) + dot(c, a));
        let corrected = angle - compensation;
        let next = sum + corrected;
        compensation = (next - sum) - corrected;
        sum = next;
    }
    sum / (4. * std::f64::consts::PI)
}
