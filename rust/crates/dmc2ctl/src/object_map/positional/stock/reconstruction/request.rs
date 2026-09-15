use super::super::super::{
    geometry::V,
    request::{scalar, vector},
    Error,
};
use crate::object_map::{model::Id, record};
pub const SCHEMA: &str = "DMC2_STOCK_MESH_REQUEST_V1";
pub const KEYS: &[&str] = &[
    "surface_analysis",
    "bounds_min_mm",
    "bounds_max_mm",
    "grid_spacing_mm",
    "normal_band_mm",
    "max_interpolation_residual_mm",
    "max_grid_vertices",
    "max_field_comparisons",
    "max_mesh_triangles",
];
pub struct Request {
    pub surface: Id,
    pub min: V,
    pub max: V,
    pub spacing: f64,
    pub band: f64,
    pub residual: f64,
    pub vertices: usize,
    pub comparisons: usize,
    pub triangles: usize,
}
impl Request {
    pub fn read(raw: &[u8]) -> Result<Self, Error> {
        let (f, payload) = record::decode(raw, SCHEMA, KEYS)?;
        if !payload.is_empty() {
            return Err(Error::Input("Stock-mesh requests use a retained surface analysis. Remove the extra payload; contact roles belong to that source analysis.".into()));
        }
        let positive = |k: &str| -> Result<f64, Error> {
            let v = scalar(&f[k], k)?;
            if v > 0. {
                Ok(v)
            } else {
                Err(Error::Input(format!(
                    "{k} must be positive. Edit the numerical reconstruction request."
                )))
            }
        };
        let budget = |k: &str| -> Result<usize, Error> {
            f[k].parse::<usize>().ok().filter(|n|*n>0).ok_or_else(||Error::Input(format!("{k} needs a positive computational budget. Edit the request; missing support will not be filled to satisfy a budget.")))
        };
        let min = vector(&f["bounds_min_mm"], "bounds_min_mm")?;
        let max = vector(&f["bounds_max_mm"], "bounds_max_mm")?;
        if (0..3).any(|i| max[i] <= min[i] || !(max[i] - min[i]).is_finite()) {
            return Err(Error::Input("Reconstruction bounds need a finite positive span on every axis. These are calculation bounds, not an inferred stock box.".into()));
        }
        Ok(Self {
            surface: Id::parse(&f["surface_analysis"])?,
            min,
            max,
            spacing: positive("grid_spacing_mm")?,
            band: positive("normal_band_mm")?,
            residual: positive("max_interpolation_residual_mm")?,
            vertices: budget("max_grid_vertices")?,
            comparisons: budget("max_field_comparisons")?,
            triangles: budget("max_mesh_triangles")?,
        })
    }
}
