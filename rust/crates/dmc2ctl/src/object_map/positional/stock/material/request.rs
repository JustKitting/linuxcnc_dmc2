use super::super::super::{request::scalar, Error};
use crate::object_map::{model::Id, record};
pub const LEGACY_SCHEMA: &str = "DMC2_MATERIAL_CHECK_REQUEST_V1";
pub const SCHEMA: &str = "DMC2_MATERIAL_CHECK_REQUEST_V2";
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
pub fn keys() -> Vec<&'static str> {
    KEYS.iter()
        .copied()
        .chain(["max_no_contact_comparisons"])
        .collect()
}
#[derive(Clone, Copy)]
pub enum EmptySpace {
    Legacy,
    RetainedSweeps { comparisons: usize },
}
impl EmptySpace {
    pub fn version(self) -> &'static str {
        match self {
            Self::Legacy => "v1",
            Self::RetainedSweeps { .. } => "v2",
        }
    }
}
pub struct Request {
    pub empty: EmptySpace,
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
        let legacy = raw.starts_with(format!("{LEGACY_SCHEMA}\n").as_bytes());
        let (f, payload) = if legacy {
            record::decode(raw, LEGACY_SCHEMA, KEYS)?
        } else {
            record::decode(raw, SCHEMA, &keys())?
        };
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
            empty: if legacy { EmptySpace::Legacy } else { EmptySpace::RetainedSweeps {
                comparisons: f["max_no_contact_comparisons"].parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(|| Error::Input("max_no_contact_comparisons requires a positive computational budget for every retained sweep and required triangle fragment. Edit this request; no pairs will be omitted.".into()))?,
            } },
            candidate: Id::parse(&f["candidate_analysis"])?, surface: Id::parse(&f["surface_analysis"])?,
            clearance: value("required_clearance_mm", false)?, allowance: value("surface_allowance_mm", false)?,
            band: value("normal_band_mm", true)?, radius: value("cover_radius_mm", true)?,
            samples: f["max_cover_samples"].parse::<usize>().ok().filter(|n| *n>0).ok_or_else(|| Error::Input("max_cover_samples needs a positive computational budget. Edit the request; no triangles will be dropped.".into()))?,
            comparisons: f["max_patch_comparisons"].parse::<usize>().ok().filter(|n| *n>0).ok_or_else(|| Error::Input("max_patch_comparisons needs a positive computational budget. Edit the request; no local comparison will be silently omitted.".into()))?,
        })
    }
}
