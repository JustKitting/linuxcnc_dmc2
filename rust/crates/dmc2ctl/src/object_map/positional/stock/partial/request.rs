use crate::object_map::{model::Id, positional::request::scalar, record, Error};
pub const SCHEMA: &str = "DMC2_PARTIAL_PLACEMENT_REQUEST_V1";
pub const KEYS: &[&str] = &[
    "material_analysis",
    "design",
    "stl_mm_per_unit",
    "translation_x_bounds_mm",
    "translation_y_bounds_mm",
    "translation_z_bounds_mm",
    "roll_correction_bounds_deg",
    "pitch_correction_bounds_deg",
    "yaw_correction_bounds_deg",
    "placement_resolution_mm",
    "max_evaluations",
    "max_patch_comparisons",
    "max_sweep_samples",
    "max_winding_terms",
];
pub struct Request {
    pub material: Id,
    pub design: Id,
    pub units: f64,
    pub lo: [f64; 6],
    pub hi: [f64; 6],
    pub resolution: f64,
    pub evaluations: usize,
    pub comparisons: usize,
    pub sweeps: usize,
    pub winding: usize,
}
impl Request {
    pub fn read(raw: &[u8]) -> Result<Self, Error> {
        let (f, payload) = record::decode(raw, SCHEMA, KEYS)?;
        if !payload.is_empty() {
            return Err(Error::Input("Partial placement uses a retained material assessment. Remove extra payload; change observation selections in a new surface analysis.".into()));
        }
        let positive = |k: &str| -> Result<f64, Error> {
            let x = scalar(&f[k], k)?;
            if x > 0. {
                Ok(x)
            } else {
                Err(Error::Input(format!(
                    "{k} must be positive. Correct the refinement request."
                )))
            }
        };
        let budget = |k: &str| {
            f[k].parse::<usize>().ok().filter(|n| *n>0).ok_or_else(|| Error::Input(format!("{k} requires a positive computation budget. Correct the request; no constraint will be omitted.")))
        };
        let mut lo = [0.; 6];
        let mut hi = [0.; 6];
        for (i, k) in KEYS[3..9].iter().enumerate() {
            let pair = f[*k].split(',').collect::<Vec<_>>();
            if pair.len() != 2 {
                return Err(Error::Input(format!(
                    "{k} needs lower,upper corrections relative to the retained placement."
                )));
            }
            let a = scalar(pair[0], k)?;
            let b = scalar(pair[1], k)?;
            if a > 0. || b < 0. || !(b - a).is_finite() || (i >= 3 && b - a > 360.) {
                return Err(Error::Input(format!("{k} must include zero correction with finite ordered bounds; rotations cannot span more than one full turn. Equal zero endpoints freeze that coordinate.")));
            }
            lo[i] = if i < 3 { a } else { a.to_radians() };
            hi[i] = if i < 3 { b } else { b.to_radians() };
        }
        let evaluations = budget("max_evaluations")?;
        if evaluations < 2 {
            return Err(Error::Input("max_evaluations must allow the original placement and domain midpoint to be evaluated. Increase the computation budget.".into()));
        }
        Ok(Self {
            material: Id::parse(&f["material_analysis"])?,
            design: Id::parse(&f["design"])?,
            units: positive("stl_mm_per_unit")?,
            lo,
            hi,
            resolution: positive("placement_resolution_mm")?,
            evaluations,
            comparisons: budget("max_patch_comparisons")?,
            sweeps: budget("max_sweep_samples")?,
            winding: budget("max_winding_terms")?,
        })
    }
}
