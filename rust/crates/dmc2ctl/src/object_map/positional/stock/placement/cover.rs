//! Triangle coverage retains the original mesh and triangle identity.
use super::super::super::{geometry::*, mesh::Triangle, Error};
pub struct Sample {
    pub triangle: usize,
    pub center: V,
    pub radius: f64,
}
pub fn build(triangles: &[Triangle], radius: f64, budget: usize) -> Result<Vec<Sample>, Error> {
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
                .map(|p| (p[0] - center[0]).hypot(p[1] - center[1]))
                .fold(0_f64, f64::max);
            if !finite(center) || !extent.is_finite() {
                return Err(Error::Data(
                    "Projected triangle coverage overflowed; inspect STL units.".into(),
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
                        let length = |i: usize| {
                            (v[i][0] - v[(i + 1) % 3][0]).hypot(v[i][1] - v[(i + 1) % 3][1])
                        };
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
