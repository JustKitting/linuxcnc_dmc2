//! Triangle coverage retains the original mesh and triangle identity.
use super::{geometry::*, mesh::Triangle, Error};
pub struct Sample {
    pub triangle: usize,
    pub center: V,
    pub radius: f64,
}
#[derive(Clone, Copy)]
pub enum Metric {
    Horizontal,
    Spatial,
}
impl Metric {
    fn distance(self, a: V, b: V) -> f64 {
        match self {
            Self::Horizontal => (a[0] - b[0]).hypot(a[1] - b[1]),
            Self::Spatial => norm(sub(a, b)),
        }
    }
}
pub fn build(
    triangles: &[Triangle],
    radius: f64,
    budget: usize,
    metric: Metric,
) -> Result<Vec<Sample>, Error> {
    let mut result = Vec::new();
    for (triangle, t) in triangles.iter().enumerate() {
        let mut pending = vec![t.v];
        while let Some(v) = pending.pop() {
            let center = add(
                v[0],
                add(
                    scale(sub(v[1], v[0]), 1. / 3.),
                    scale(sub(v[2], v[0]), 1. / 3.),
                ),
            );
            let extent = v
                .iter()
                .map(|p| metric.distance(*p, center))
                .fold(0_f64, f64::max);
            if !finite(center) || !extent.is_finite() {
                return Err(Error::Data(
                    "Triangle coverage overflowed; inspect STL units.".into(),
                ));
            }
            if extent <= radius {
                result.push(Sample {
                    triangle,
                    center,
                    radius: extent,
                });
            } else {
                let i = (0..3)
                    .max_by(|a, b| {
                        let length = |i: usize| metric.distance(v[i], v[(i + 1) % 3]);
                        length(*a).total_cmp(&length(*b))
                    })
                    .unwrap();
                let (a, b, c) = (v[i], v[(i + 1) % 3], v[(i + 2) % 3]);
                let m = add(a, scale(sub(b, a), 0.5));
                if m == a || m == b {
                    return Err(Error::Data("cover_radius_mm is below representable triangle spacing. Increase the computational cover radius; the mesh was not altered.".into()));
                }
                pending.push([a, m, c]);
                pending.push([m, b, c]);
            }
            if result
                .len()
                .saturating_add(pending.len())
                .saturating_add(triangles.len() - triangle - 1)
                > budget
            {
                return Err(Error::Input("max_cover_samples cannot cover every source triangle at cover_radius_mm. Increase the computational budget or choose a coarser explicit radius; no geometry was dropped.".into()));
            }
        }
    }
    Ok(result)
}
