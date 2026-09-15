//! Explicit enclosed material model for one closed, embedded oriented shell.
//! This checks geometry; it does not establish the physical absence of cavities.
use super::super::geometry::*;
use super::{Error, Mesh, V, data, intersection};
use std::collections::{BTreeMap, BTreeSet};

type Key = [u64; 3];
fn key(p: V) -> Key {
    p.map(|x| if x == 0. { 0 } else { x.to_bits() })
}
pub struct Solid<'a> {
    mesh: &'a Mesh,
    pub topology_visits: usize,
    pub triangle_pairs: usize,
    pub vertices: usize,
}
pub struct Distance {
    /// Positive inward distance, negative outside, zero on the boundary.
    pub inward: f64,
    pub triangle: usize,
    pub winding: Option<f64>,
}
impl<'a> Solid<'a> {
    pub fn new(mesh: &'a Mesh, max_visits: usize) -> Result<Self, Error> {
        if mesh.boundary_edges != 0 || mesh.inconsistent_edges != 0 || mesh.signed_volume <= 0. {
            return Err(data(format!(
                "The enclosed-stock model needs a closed outward shell; this mesh has {} open edges, {} inconsistent edges and signed volume {} mm3. Inspect reconstruction measurement needs and resolve the source boundary; no holes were capped or unknown volume filled.",
                mesh.boundary_edges, mesh.inconsistent_edges, mesh.signed_volume
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
                    "The stock boundary pinches or branches at vertex {}. Resolve the source geometry and reconstruct a new mesh; an enclosed solid was not accepted.",
                    csv(vertex.map(f64::from_bits))
                )));
            }
        }
        let mut adjacency = vec![Vec::new(); mesh.triangles.len()];
        for faces in edges.values() {
            adjacency[faces[0]].push(faces[1]);
            adjacency[faces[1]].push(faces[0]);
        }
        let mut seen = BTreeSet::new();
        let mut pending = vec![0];
        while let Some(i) = pending.pop() {
            if seen.insert(i) {
                pending.extend(&adjacency[i]);
            }
        }
        if seen.len() != mesh.triangles.len() {
            return Err(data(
                "The stock mesh contains multiple disconnected shells. Their material/cavity relationship is not defined by single-shell-enclosed-solid. Resolve that stock model explicitly; no shell was discarded or filled.",
            ));
        }
        let mut triangle_pairs = 0;
        let topology_visits = mesh.spatial.overlap_pairs(&mesh.triangles, max_visits, |a,b| {
            triangle_pairs += 1;
            if intersection::beyond_shared_simplex(&mesh.triangles[a], &mesh.triangles[b])? {
                return Err(data(format!("Stock triangles {a} and {b} intersect beyond a shared edge or vertex. Inspect their retained reconstruction sources and resolve the geometry; no intersection repair was applied.")));
            }
            Ok(())
        })?;
        Ok(Self {
            mesh,
            topology_visits,
            triangle_pairs,
            vertices: vertices.len(),
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
        let (mut sum, mut compensation) = (0., 0.);
        for t in &self.mesh.triangles {
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
        let winding = sum / (4. * std::f64::consts::PI);
        if !winding.is_finite() || winding == 0.5 {
            return Err(data(
                "The enclosed-stock query has an indeterminate winding value. Preserve this analysis input and inspect the stock geometry/coordinate scale; no inside/outside result was substituted.",
            ));
        }
        Ok(Distance {
            inward: if winding > 0.5 { distance } else { -distance },
            triangle: nearest.triangle,
            winding: Some(winding),
        })
    }
}
