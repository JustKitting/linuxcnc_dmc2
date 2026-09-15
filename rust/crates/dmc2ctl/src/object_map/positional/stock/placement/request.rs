use super::super::super::{request::scalar, Error};
use crate::object_map::{model::Id, record};
pub const SCHEMA: &str = "DMC2_FOOTPRINT_REQUEST_V1";
pub const KEYS: &[&str] = &[
    "outline_analysis",
    "design",
    "required_geometry_role",
    "stl_mm_per_unit",
    "model_origin_z_mm",
    "model_origin_x_bounds_mm",
    "model_origin_y_bounds_mm",
    "yaw_bounds_deg",
    "required_clearance_mm",
    "cover_radius_mm",
    "max_cover_samples",
    "placement_resolution_mm",
    "max_evaluations",
];
pub struct Request {
    pub outline: Id,
    pub design: Id,
    pub units: f64,
    pub z: f64,
    pub lo: [f64; 3],
    pub hi: [f64; 3],
    pub margin: f64,
    pub radius: f64,
    pub samples: usize,
    pub resolution: f64,
    pub evaluations: usize,
}
impl Request {
    pub fn read(raw: &[u8]) -> Result<Self, Error> {
        let (f, payload) = record::decode(raw, SCHEMA, KEYS)?;
        if !payload.is_empty() {
            return Err(Error::Input("Footprint requests reference a retained outline analysis; contact selections belong to that source request. Remove the extra payload.".into()));
        }
        if f["required_geometry_role"] != "operation-retained-material" {
            return Err(Error::Input("required_geometry_role must be operation-retained-material. Select the complete material that this operation must preserve, including its backing and holding features.".into()));
        }
        let positive = |key: &str| -> Result<f64, Error> {
            let v = scalar(&f[key], key)?;
            if v > 0. {
                Ok(v)
            } else {
                Err(Error::Input(format!(
                    "{key} must be positive; edit this analysis request."
                )))
            }
        };
        let budget = |key: &str| -> Result<usize, Error> {
            f[key]
                .parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| {
                    Error::Input(format!(
                        "{key} needs a positive integer computational budget; edit the request."
                    ))
                })
        };
        let bounds = |key: &str| -> Result<[f64; 2], Error> {
            let parts = f[key].split(',').collect::<Vec<_>>();
            if parts.len() != 2 {
                return Err(Error::Input(format!(
                    "{key} needs lower,upper. Equal endpoints keep that coordinate fixed."
                )));
            }
            let p = [scalar(parts[0], key)?, scalar(parts[1], key)?];
            if p[1] < p[0] || !(p[1] - p[0]).is_finite() {
                return Err(Error::Input(format!(
                    "{key} needs finite ordered bounds; correct the request."
                )));
            }
            Ok(p)
        };
        let [x, y, yaw] = [
            bounds("model_origin_x_bounds_mm")?,
            bounds("model_origin_y_bounds_mm")?,
            bounds("yaw_bounds_deg")?,
        ];
        if yaw[1] - yaw[0] > 360. {
            return Err(Error::Input("yaw_bounds_deg cannot span more than a full turn; repeated orientations add no placement choices.".into()));
        }
        let margin = scalar(&f["required_clearance_mm"], "required_clearance_mm")?;
        if margin < 0. {
            return Err(Error::Input(
                "required_clearance_mm must be nonnegative; a material deficit is not clearance."
                    .into(),
            ));
        }
        Ok(Self {
            outline: Id::parse(&f["outline_analysis"])?,
            design: Id::parse(&f["design"])?,
            units: positive("stl_mm_per_unit")?,
            z: scalar(&f["model_origin_z_mm"], "model_origin_z_mm")?,
            lo: [x[0], y[0], yaw[0].to_radians()],
            hi: [x[1], y[1], yaw[1].to_radians()],
            margin,
            radius: positive("cover_radius_mm")?,
            samples: budget("max_cover_samples")?,
            resolution: positive("placement_resolution_mm")?,
            evaluations: budget("max_evaluations")?,
        })
    }
}
