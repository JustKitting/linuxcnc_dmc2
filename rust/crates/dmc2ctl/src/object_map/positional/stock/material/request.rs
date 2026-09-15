use super::super::super::{request::scalar, Error};
use crate::object_map::{model::Id, record};
pub const SCHEMA: &str = "DMC2_MATERIAL_CHECK_REQUEST_V1";
pub const KEYS: &[&str] = &[
    "candidate_analysis",
    "surface_analysis",
    "required_clearance_mm",
    "surface_allowance_mm",
    "normal_band_mm",
    "cover_radius_mm",
    "max_cover_samples",
    "max_patch_comparisons",
];
pub struct Request {
    pub candidate: Id,
    pub surface: Id,
    pub clearance: f64,
    pub allowance: f64,
    pub band: f64,
    pub radius: f64,
    pub samples: usize,
    pub comparisons: usize,
}
impl Request {
    pub fn read(raw: &[u8]) -> Result<Self, Error> {
        let (f, payload) = record::decode(raw, SCHEMA, KEYS)?;
        if !payload.is_empty() {
            return Err(Error::Input("Material checks use retained source analyses. Remove the extra payload; edit contact roles in a new source-surface analysis.".into()));
        }
        let value = |key: &str, positive: bool| -> Result<f64, Error> {
            let v = scalar(&f[key], key)?;
            if v < 0. || positive && v == 0. {
                return Err(Error::Input(format!(
                    "{key} must be {}. Edit this analysis request.",
                    if positive { "positive" } else { "nonnegative" }
                )));
            }
            Ok(v)
        };
        Ok(Self {
            candidate: Id::parse(&f["candidate_analysis"])?, surface: Id::parse(&f["surface_analysis"])?,
            clearance: value("required_clearance_mm", false)?, allowance: value("surface_allowance_mm", false)?,
            band: value("normal_band_mm", true)?, radius: value("cover_radius_mm", true)?,
            samples: f["max_cover_samples"].parse::<usize>().ok().filter(|n| *n>0).ok_or_else(|| Error::Input("max_cover_samples needs a positive computational budget. Edit the request; no triangles will be dropped.".into()))?,
            comparisons: f["max_patch_comparisons"].parse::<usize>().ok().filter(|n| *n>0).ok_or_else(|| Error::Input("max_patch_comparisons needs a positive computational budget. Edit the request; no local comparison will be silently omitted.".into()))?,
        })
    }
}
