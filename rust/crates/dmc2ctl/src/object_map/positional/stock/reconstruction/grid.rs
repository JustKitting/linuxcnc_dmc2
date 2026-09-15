//! Shared lattice indices make adjacent tetrahedra use the same edge samples.
use super::super::super::{geometry::V, Error};
use super::request::Request;
pub struct Grid {
    pub axes: [Vec<f64>; 3],
    pub count: usize,
}
impl Grid {
    pub fn new(r: &Request) -> Result<Self, Error> {
        let mut sizes = [0usize; 3];
        let mut count = 1usize;
        for (i, size) in sizes.iter_mut().enumerate() {
            let intervals = ((r.max[i] - r.min[i]) / r.spacing).ceil().max(1.);
            if !intervals.is_finite() || intervals < 1. || intervals >= usize::MAX as f64 {
                return Err(Error::Input("The requested grid cannot be indexed at this spacing. Coarsen its explicit spacing or narrow the calculation bounds.".into()));
            }
            *size = (intervals as usize).checked_add(1).ok_or_else(|| {
                Error::Input(
                    "Grid axis count overflowed. Coarsen the spacing or narrow its bounds.".into(),
                )
            })?;
            count = count.checked_mul(*size).ok_or_else(|| {
                Error::Input(
                    "Grid vertex count overflowed. Coarsen the spacing or narrow its bounds."
                        .into(),
                )
            })?;
        }
        if count > r.vertices {
            return Err(Error::Input(format!("This grid needs {count} vertices, above max_grid_vertices={}. Increase the explicit computational budget or coarsen/narrow the grid; no samples were omitted.",r.vertices)));
        }
        let mut axes: [Vec<f64>; 3] = std::array::from_fn(|_| Vec::new());
        for i in 0..3 {
            for j in 0..sizes[i] {
                let p = if j == sizes[i] - 1 {
                    r.max[i]
                } else {
                    r.min[i] + (r.max[i] - r.min[i]) * (j as f64 / (sizes[i] - 1) as f64)
                };
                if !p.is_finite() || axes[i].last().is_some_and(|last| p <= *last) {
                    return Err(Error::Input("Grid spacing is below representable coordinate separation. Coarsen the numerical grid; no coincident nodes were merged.".into()));
                }
                axes[i].push(p);
            }
        }
        Ok(Self { axes, count })
    }
    pub fn index(&self, p: [usize; 3]) -> usize {
        p[0] + self.axes[0].len() * (p[1] + self.axes[1].len() * p[2])
    }
    pub fn point(&self, index: usize) -> V {
        let x = index % self.axes[0].len();
        let yz = index / self.axes[0].len();
        [
            self.axes[0][x],
            self.axes[1][yz % self.axes[1].len()],
            self.axes[2][yz / self.axes[1].len()],
        ]
    }
    pub fn cube(&self, p: [usize; 3]) -> [usize; 8] {
        std::array::from_fn(|i| {
            self.index([p[0] + (i & 1), p[1] + ((i >> 1) & 1), p[2] + ((i >> 2) & 1)])
        })
    }
}
