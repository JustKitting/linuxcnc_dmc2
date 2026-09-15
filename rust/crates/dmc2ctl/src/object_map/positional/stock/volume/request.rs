use super::super::super::{Error, request::scalar};
use crate::object_map::{model::Id, record};
pub const SCHEMA: &str = "DMC2_VOLUME_PLACEMENT_REQUEST_V1";
pub const KEYS: &[&str] = &[
    "stock_mesh_analysis",
    "design",
    "required_geometry_role",
    "stock_occupancy_model",
    "stl_mm_per_unit",
    "model_origin_x_bounds_mm",
    "model_origin_y_bounds_mm",
    "model_origin_z_bounds_mm",
    "roll_bounds_deg",
    "pitch_bounds_deg",
    "yaw_bounds_deg",
    "required_clearance_mm",
    "surface_allowance_mm",
    "cover_radius_mm",
    "max_cover_samples",
    "placement_resolution_mm",
    "max_evaluations",
    "max_topology_visits",
    "max_winding_terms",
];
#[derive(Clone, Copy)]
pub enum Occupancy {
    SingleShellEnclosedSolid,
}
impl Occupancy {
    pub fn name(self) -> &'static str {
        match self {
            Self::SingleShellEnclosedSolid => "single-shell-enclosed-solid",
        }
    }
    fn read(s: &str) -> Result<Self, Error> {
        match s {
            "single-shell-enclosed-solid" => Ok(Self::SingleShellEnclosedSolid),
            _ => Err(Error::Input("stock_occupancy_model must explicitly select single-shell-enclosed-solid: the inside of one reconstructed closed shell is modeled as material without hidden cavities. This does not assert the physical stock has that property. Open or multiple shells require further stock modeling; no implicit filling is available.".into())),
        }
    }
}
pub struct Request {
    pub stock: Id,
    pub design: Id,
    pub occupancy: Occupancy,
    pub units: f64,
    /// Translation in mm, then roll/pitch/yaw radians (Rz Ry Rx).
    pub lo: [f64; 6],
    pub hi: [f64; 6],
    pub margin: f64,
    pub allowance: f64,
    pub radius: f64,
    pub samples: usize,
    pub resolution: f64,
    pub evaluations: usize,
    pub topology_visits: usize,
    pub winding_terms: usize,
}
impl Request {
    pub fn read(raw: &[u8]) -> Result<Self, Error> {
        let (f, payload) = record::decode(raw, SCHEMA, KEYS)?;
        if !payload.is_empty() {
            return Err(Error::Input("A volume placement request uses retained source analyses. Remove its extra payload; contact selections belong to the source surface request.".into()));
        }
        if f["required_geometry_role"] != "operation-retained-material" {
            return Err(Error::Input("required_geometry_role must be operation-retained-material. Select the full geometry the operation must preserve, including backing and holding features.".into()));
        }
        let nonnegative = |k: &str| -> Result<f64, Error> {
            let v = scalar(&f[k], k)?;
            if v >= 0. {
                Ok(v)
            } else {
                Err(Error::Input(format!(
                    "{k} must be nonnegative; edit this request."
                )))
            }
        };
        let positive = |k: &str| -> Result<f64, Error> {
            let v = scalar(&f[k], k)?;
            if v > 0. {
                Ok(v)
            } else {
                Err(Error::Input(format!(
                    "{k} must be positive; edit this request."
                )))
            }
        };
        let budget = |k: &str| -> Result<usize, Error> {
            f[k].parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| {
                    Error::Input(format!(
                        "{k} needs a positive integer computation budget; edit this request."
                    ))
                })
        };
        let mut lo = [0.; 6];
        let mut hi = [0.; 6];
        for (i, k) in [
            "model_origin_x_bounds_mm",
            "model_origin_y_bounds_mm",
            "model_origin_z_bounds_mm",
            "roll_bounds_deg",
            "pitch_bounds_deg",
            "yaw_bounds_deg",
        ]
        .iter()
        .enumerate()
        {
            let p = f[*k].split(',').collect::<Vec<_>>();
            if p.len() != 2 {
                return Err(Error::Input(format!(
                    "{k} needs lower,upper. Equal endpoints keep that coordinate fixed."
                )));
            }
            let a = scalar(p[0], k)?;
            let b = scalar(p[1], k)?;
            if b < a || !(b - a).is_finite() || (i >= 3 && b - a > 360.) {
                return Err(Error::Input(format!(
                    "{k} needs ordered finite bounds; rotation intervals cannot exceed a full turn. Correct the request."
                )));
            }
            // Reduce the common full-turn offset before radians conversion so
            // large equivalent angles do not degrade trigonometric precision.
            if i >= 3 {
                lo[i] = a.rem_euclid(360.).to_radians();
                hi[i] = lo[i] + (b - a).to_radians();
            } else {
                lo[i] = a;
                hi[i] = b;
            }
        }
        Ok(Self {
            stock: Id::parse(&f["stock_mesh_analysis"])?,
            design: Id::parse(&f["design"])?,
            occupancy: Occupancy::read(&f["stock_occupancy_model"])?,
            units: positive("stl_mm_per_unit")?,
            lo,
            hi,
            margin: nonnegative("required_clearance_mm")?,
            allowance: nonnegative("surface_allowance_mm")?,
            radius: positive("cover_radius_mm")?,
            samples: budget("max_cover_samples")?,
            resolution: positive("placement_resolution_mm")?,
            evaluations: budget("max_evaluations")?,
            topology_visits: budget("max_topology_visits")?,
            winding_terms: budget("max_winding_terms")?,
        })
    }
}
