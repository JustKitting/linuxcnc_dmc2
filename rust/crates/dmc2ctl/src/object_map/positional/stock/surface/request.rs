use super::super::super::{
    probe::Probe,
    request::{read_selections, scalar, Selection, Use},
    Error,
};
use crate::object_map::record;
pub const SCHEMA: &str = "DMC2_STOCK_SURFACE_REQUEST_V1";
pub fn keys() -> Vec<&'static str> {
    super::super::super::probe::keys(&[
        "neighborhood_mm",
        "max_approach_angle_deg",
        "huber_mm",
        "max_iterations",
        "convergence_mm",
        "max_support_gap_mm",
        "max_fit_residual_mm",
    ])
}
pub struct Request {
    pub probe: Probe,
    pub neighborhood: f64,
    pub approach_cos: f64,
    pub huber: f64,
    pub iterations: usize,
    pub convergence: f64,
    pub support_gap: f64,
    pub max_residual: f64,
    pub selected: Vec<Selection>,
}
impl Request {
    pub fn read(raw: &[u8]) -> Result<Self, Error> {
        let (f, body) = record::decode(raw, SCHEMA, &keys())?;
        let positive = |k: &str| -> Result<f64, Error> {
            let v = scalar(&f[k], k)?;
            if v > 0. {
                Ok(v)
            } else {
                Err(Error::Input(format!(
                    "{k} must be positive; edit this analysis request."
                )))
            }
        };
        let angle = positive("max_approach_angle_deg")?;
        if angle > 180. {
            return Err(Error::Input("max_approach_angle_deg cannot exceed a half-turn; edit the neighborhood selection in this request.".into()));
        }
        let iterations = f["max_iterations"].parse::<usize>().ok().filter(|n|*n>0)
            .ok_or_else(|| Error::Input("max_iterations needs a positive integer computational budget; edit the request.".into()))?;
        let selected = read_selections(body)?;
        if selected.iter().any(|s| matches!(s.usage, Use::Face(..))) {
            return Err(Error::Input("3D stock surface rows use fit, check or observe. Named box faces do not define this surface model.".into()));
        }
        if selected.iter().filter(|s| s.usage == Use::Fit).count() < 3 {
            return Err(Error::Input("Select at least three fine contacts for local plane fitting; each neighborhood also needs noncollinear support. Keep independent check contacts.".into()));
        }
        Ok(Self {
            probe: Probe::read(&f)?,
            neighborhood: positive("neighborhood_mm")?,
            approach_cos: angle.to_radians().cos(),
            huber: positive("huber_mm")?,
            iterations,
            convergence: positive("convergence_mm")?,
            support_gap: positive("max_support_gap_mm")?,
            max_residual: positive("max_fit_residual_mm")?,
            selected,
        })
    }
}
