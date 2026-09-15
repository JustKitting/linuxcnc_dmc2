use super::super::super::{request::scalar, Error};
use crate::object_map::{model::Id, record};
pub const LEGACY_SCHEMA: &str = "DMC2_MATERIAL_CHECK_REQUEST_V1";
pub const SWEEP_SCHEMA: &str = "DMC2_MATERIAL_CHECK_REQUEST_V2";
pub const SCHEMA: &str = "DMC2_MATERIAL_CHECK_REQUEST_V3";
pub const OCCUPANCY: &str = "oriented-material-boundary";
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
    sweep_keys()
        .into_iter()
        .chain([
            "required_occupancy_model",
            "max_topology_visits",
            "max_winding_terms",
        ])
        .collect()
}
pub fn sweep_keys() -> Vec<&'static str> {
    KEYS.iter()
        .copied()
        .chain(["max_no_contact_comparisons"])
        .collect()
}
#[derive(Clone, Copy)]
pub enum EmptySpace {
    Legacy,
    RetainedSweeps {
        comparisons: usize,
    },
    RequiredVolume {
        comparisons: usize,
        topology_visits: usize,
        winding_terms: usize,
    },
}
impl EmptySpace {
    pub fn version(self) -> &'static str {
        match self {
            Self::Legacy => "v1",
            Self::RetainedSweeps { .. } => "v2",
            Self::RequiredVolume { .. } => "v3",
        }
    }
    pub fn comparisons(self) -> Option<usize> {
        match self {
            Self::Legacy => None,
            Self::RetainedSweeps { comparisons } | Self::RequiredVolume { comparisons, .. } => {
                Some(comparisons)
            }
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
        let sweeps = raw.starts_with(format!("{SWEEP_SCHEMA}\n").as_bytes());
        let (f, payload) = if legacy {
            record::decode(raw, LEGACY_SCHEMA, KEYS)?
        } else if sweeps {
            record::decode(raw, SWEEP_SCHEMA, &sweep_keys())?
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
        let budget = |key: &str| {
            f[key].parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(|| Error::Input(format!("{key} requires a positive computation budget. Edit this request; no source geometry or query will be skipped.")))
        };
        let empty = if legacy {
            EmptySpace::Legacy
        } else {
            let comparisons = f["max_no_contact_comparisons"].parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(|| Error::Input("max_no_contact_comparisons requires a positive computational budget for every retained sweep and required triangle fragment. Edit this request; no pairs will be omitted.".into()))?;
            if sweeps {
                EmptySpace::RetainedSweeps { comparisons }
            } else {
                if f["required_occupancy_model"] != OCCUPANCY {
                    return Err(Error::Input(format!("required_occupancy_model must explicitly select {OCCUPANCY}: the unchanged required-operation STL bounds material with outward exterior shells and oppositely oriented cavity shells. This declares required geometry, not measured stock occupancy. Correct the request or use a surface-only material request for an open reference mesh.")));
                }
                EmptySpace::RequiredVolume {
                    comparisons,
                    topology_visits: budget("max_topology_visits")?,
                    winding_terms: budget("max_winding_terms")?,
                }
            }
        };
        Ok(Self {
            empty,
            candidate: Id::parse(&f["candidate_analysis"])?, surface: Id::parse(&f["surface_analysis"])?,
            clearance: value("required_clearance_mm", false)?, allowance: value("surface_allowance_mm", false)?,
            band: value("normal_band_mm", true)?, radius: value("cover_radius_mm", true)?,
            samples: f["max_cover_samples"].parse::<usize>().ok().filter(|n| *n>0).ok_or_else(|| Error::Input("max_cover_samples needs a positive computational budget. Edit the request; no triangles will be dropped.".into()))?,
            comparisons: f["max_patch_comparisons"].parse::<usize>().ok().filter(|n| *n>0).ok_or_else(|| Error::Input("max_patch_comparisons needs a positive computational budget. Edit the request; no local comparison will be silently omitted.".into()))?,
        })
    }
}
