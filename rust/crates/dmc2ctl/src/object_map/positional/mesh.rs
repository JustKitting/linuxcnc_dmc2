//! ASCII/binary STL triangles. Explicit units; geometric normals from winding.
use super::{super::Error, geometry::*};
use std::collections::BTreeMap;
#[derive(Clone, Copy)]
pub struct Triangle {
    pub v: [V; 3],
    pub n: V,
}
pub struct Mesh {
    pub triangles: Vec<Triangle>,
    pub min: V,
    pub max: V,
    pub boundary_edges: usize,
    pub inconsistent_edges: usize,
    pub signed_volume: f64,
}
pub struct Nearest {
    pub triangle: usize,
    pub point: V,
    pub normal: V,
    pub distance: f64,
}
impl Triangle {
    fn new(v: [V; 3]) -> Result<Self, Error> {
        let c = cross(sub(v[1], v[0]), sub(v[2], v[0]));
        let l = norm(c);
        if !v.iter().all(|p| finite(*p)) || !l.is_finite() || l == 0. {
            return Err(Error::Data("STL contains a nonfinite or degenerate triangle; repair the source geometry and attach a new revision.".into()));
        }
        Ok(Self {
            v,
            n: scale(c, 1. / l),
        })
    }
    fn nearest(self, p: V) -> V {
        // Voronoi regions of the triangle, including edges/vertices.
        let [a, b, c] = self.v;
        let ab = sub(b, a);
        let ac = sub(c, a);
        let ap = sub(p, a);
        let d1 = dot(ab, ap);
        let d2 = dot(ac, ap);
        if d1 <= 0. && d2 <= 0. {
            return a;
        }
        let bp = sub(p, b);
        let d3 = dot(ab, bp);
        let d4 = dot(ac, bp);
        if d3 >= 0. && d4 <= d3 {
            return b;
        }
        let vc = d1 * d4 - d3 * d2;
        if vc <= 0. && d1 >= 0. && d3 <= 0. {
            return add(a, scale(ab, d1 / (d1 - d3)));
        }
        let cp = sub(p, c);
        let d5 = dot(ab, cp);
        let d6 = dot(ac, cp);
        if d6 >= 0. && d5 <= d6 {
            return c;
        }
        let vb = d5 * d2 - d1 * d6;
        if vb <= 0. && d2 >= 0. && d6 <= 0. {
            return add(a, scale(ac, d2 / (d2 - d6)));
        }
        let va = d3 * d6 - d5 * d4;
        if va <= 0. && d4 - d3 >= 0. && d5 - d6 >= 0. {
            return add(b, scale(sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6))));
        }
        let denominator = va + vb + vc;
        add(
            a,
            add(scale(ab, vb / denominator), scale(ac, vc / denominator)),
        )
    }
}
fn data(s: impl Into<String>) -> Error {
    Error::Data(s.into())
}
fn line<'a>(lines: &mut impl Iterator<Item = &'a str>, expected: &str) -> Result<(), Error> {
    if lines.next().map(str::trim) != Some(expected) {
        return Err(data(format!("ASCII STL expected {expected:?}.")));
    }
    Ok(())
}
impl Mesh {
    pub fn read(raw: &[u8], units: f64) -> Result<Self, Error> {
        if !units.is_finite() || units <= 0. {
            return Err(Error::Input(
                "STL requires explicit positive millimetres per file unit.".into(),
            ));
        }
        let count = raw
            .get(80..84)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
        let binary = count
            .and_then(|n| n.checked_mul(50)?.checked_add(84))
            .is_some_and(|n| n == raw.len());
        let mut triangles = Vec::new();
        if binary {
            for facet in raw[84..].chunks_exact(50) {
                let mut v = [[0.; 3]; 3];
                for (i, p) in v.iter_mut().enumerate() {
                    for (j, x) in p.iter_mut().enumerate() {
                        let k = 12 + i * 12 + j * 4;
                        *x = f32::from_le_bytes(facet[k..k + 4].try_into().unwrap()) as f64 * units;
                    }
                }
                triangles.push(Triangle::new(v)?);
            }
        } else {
            let text = std::str::from_utf8(raw).map_err(|_| {
                data("STL is neither an exact-length binary STL nor UTF-8 ASCII STL.")
            })?;
            let mut lines = text.lines().filter(|l| !l.trim().is_empty());
            if !lines
                .next()
                .is_some_and(|l| l.trim() == "solid" || l.trim().starts_with("solid "))
            {
                return Err(data(
                    "ASCII STL needs its solid header; binary STL length may be corrupt.",
                ));
            }
            let mut closed = false;
            while let Some(l) = lines.next() {
                let l = l.trim();
                if l == "endsolid" || l.starts_with("endsolid ") {
                    closed = true;
                    break;
                }
                let n = l
                    .strip_prefix("facet normal ")
                    .ok_or_else(|| data("ASCII STL expected facet normal."))?;
                let numbers = n.split_whitespace().collect::<Vec<_>>();
                if numbers.len() != 3
                    || numbers
                        .iter()
                        .any(|x| x.parse::<f64>().ok().is_none_or(|v| !v.is_finite()))
                {
                    return Err(data("ASCII STL facet normal is invalid."));
                }
                line(&mut lines, "outer loop")?;
                let mut v = [[0.; 3]; 3];
                for p in &mut v {
                    let l = lines
                        .next()
                        .ok_or_else(|| data("STL triangle is truncated."))?
                        .trim();
                    let xyz = l
                        .strip_prefix("vertex ")
                        .ok_or_else(|| data("ASCII STL expected a vertex."))?
                        .split_whitespace()
                        .collect::<Vec<_>>();
                    if xyz.len() != 3 {
                        return Err(data("STL vertex needs three coordinates."));
                    }
                    for (j, x) in xyz.iter().enumerate() {
                        *p.get_mut(j).unwrap() = x
                            .parse::<f64>()
                            .map_err(|e| data(format!("STL coordinate: {e}.")))?
                            * units;
                    }
                }
                line(&mut lines, "endloop")?;
                line(&mut lines, "endfacet")?;
                triangles.push(Triangle::new(v)?);
            }
            if !closed || lines.next().is_some() {
                return Err(data(
                    "ASCII STL has no terminal endsolid or has trailing geometry.",
                ));
            }
        }
        if triangles.is_empty() {
            return Err(data("STL contains no triangles."));
        }
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        let mut edges = BTreeMap::<([u64; 3], [u64; 3]), (usize, i64)>::new();
        for t in &triangles {
            for p in t.v {
                for i in 0..3 {
                    min[i] = min[i].min(p[i]);
                    max[i] = max[i].max(p[i]);
                }
            }
            for i in 0..3 {
                let a = t.v[i].map(|v| if v == 0. { 0 } else { v.to_bits() });
                let b = t.v[(i + 1) % 3].map(|v| if v == 0. { 0 } else { v.to_bits() });
                let (key, sign) = if a < b { ((a, b), 1) } else { ((b, a), -1) };
                let e = edges.entry(key).or_default();
                e.0 += 1;
                e.1 += sign;
            }
        }
        let origin = scale(add(min, max), 0.5);
        let signed_volume = triangles
            .iter()
            .map(|t| {
                dot(
                    sub(t.v[0], origin),
                    cross(sub(t.v[1], origin), sub(t.v[2], origin)),
                ) / 6.
            })
            .sum();
        let boundary_edges = edges.values().filter(|(n, _)| *n == 1).count();
        let inconsistent_edges = edges
            .values()
            .filter(|(n, s)| *n > 2 || (*n == 2 && *s != 0))
            .count();
        if !norm(sub(max, min)).is_finite() || !f64::is_finite(signed_volume) {
            return Err(data(
                "STL geometry overflows millimetre calculations; check its units.",
            ));
        }
        Ok(Self {
            triangles,
            min,
            max,
            boundary_edges,
            inconsistent_edges,
            signed_volume,
        })
    }
    pub fn fitting_geometry(&self) -> Result<(), Error> {
        if self.boundary_edges != 0 || self.inconsistent_edges != 0 || self.signed_volume <= 0. {
            return Err(data(format!("STL does not have closed consistent outward winding (boundary edges {}, inconsistent edges {}, signed volume {}). Repair the mesh before sphere-to-surface fitting; inspection results remain available.",self.boundary_edges,self.inconsistent_edges,self.signed_volume)));
        }
        Ok(())
    }
    pub fn nearest(&self, p: V) -> Result<Nearest, Error> {
        let mut best = None;
        for (i, t) in self.triangles.iter().enumerate() {
            let q = t.nearest(p);
            let delta = sub(p, q);
            let distance = norm(delta);
            if !finite(q) || !distance.is_finite() {
                return Err(data("Closest-triangle calculation overflowed; check STL units and initial placement."));
            }
            if best
                .as_ref()
                .is_none_or(|b: &Nearest| distance < b.distance.abs())
            {
                let sign = if dot(delta, t.n) < 0. { -1. } else { 1. };
                let normal = if distance == 0. {
                    t.n
                } else {
                    scale(delta, sign / distance)
                };
                best = Some(Nearest {
                    triangle: i,
                    point: q,
                    normal,
                    distance: sign * distance,
                });
            }
        }
        best.ok_or_else(|| data("STL has no searchable geometry."))
    }
    pub fn json(&self) -> String {
        format!("{{\"triangles\":{},\"min_mm\":{},\"max_mm\":{},\"span_mm\":{},\"boundary_edges\":{},\"inconsistent_edges\":{},\"signed_volume_mm3\":{},\"self_intersections_checked\":false}}",self.triangles.len(),json(self.min),json(self.max),json(sub(self.max,self.min)),self.boundary_edges,self.inconsistent_edges,self.signed_volume)
    }
    pub fn transformed_stl(&self, pose: Pose) -> Result<String, Error> {
        let mut text = String::from("solid dmc2_pose_candidate_mm\n");
        for t in &self.triangles {
            let n = mv(pose.r, t.n);
            text.push_str(&format!(
                "facet normal {}\nouter loop\n",
                csv(n).replace(',', " ")
            ));
            for p in t.v {
                let p = pose.point(p);
                if !finite(p) {
                    return Err(data("Transformed STL overflows; inspect the placement."));
                }
                text.push_str(&format!("vertex {}\n", csv(p).replace(',', " ")));
            }
            text.push_str("endloop\nendfacet\n");
        }
        text.push_str("endsolid dmc2_pose_candidate_mm\n");
        Ok(text)
    }
}
