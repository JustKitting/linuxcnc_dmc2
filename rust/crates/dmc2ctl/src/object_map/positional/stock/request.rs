use super::super::{
    probe::Probe,
    request::{read_selections, scalar, Selection, Use},
    Error,
};
use crate::object_map::record;
pub const SCHEMA: &str = "DMC2_STOCK_OUTLINE_REQUEST_V1";
pub const KEYS: &[&str] = &[
    "calibration_state",
    "calibration_reference",
    "frame_reference",
    "ball_radius_mm",
    "trigger_to_ball_mm",
    "pretravel_mm",
    "closure",
    "surface_model",
    "neighborhood_span_mm",
    "huber_mm",
    "max_iterations",
    "convergence_mm",
    "max_gap_mm",
    "max_z_span_mm",
    "max_fit_residual_mm",
];
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Closure {
    Open,
    Closed,
}
impl Closure {
    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    ProbeCentres,
    VerticalSides,
}
impl Surface {
    pub fn name(self) -> &'static str {
        match self {
            Self::ProbeCentres => "probe-centres",
            Self::VerticalSides => "vertical-sides",
        }
    }
}
pub struct Request {
    pub probe: Probe,
    pub closure: Closure,
    pub surface: Surface,
    pub span: f64,
    pub huber: f64,
    pub iterations: usize,
    pub convergence: f64,
    pub max_gap: f64,
    pub max_z_span: f64,
    pub max_residual: f64,
    pub selected: Vec<Selection>,
}
impl Request {
    pub fn read(raw: &[u8]) -> Result<Self, Error> {
        let (f, body) = record::decode(raw, SCHEMA, KEYS)?;
        let positive = |key: &str| -> Result<f64, Error> {
            let x = scalar(&f[key], key)?;
            if x <= 0. {
                Err(Error::Input(format!("{key} must be positive.")))
            } else {
                Ok(x)
            }
        };
        let closure = match f["closure"].as_str() {
            "open" => Closure::Open, "closed" => Closure::Closed,
            _ => return Err(Error::Input("closure must be open or closed. Closed declares the selected traversal topology; it does not certify seam agreement or material coverage.".into())),
        };
        let surface = match f["surface_model"].as_str() {
            "probe-centres" => Surface::ProbeCentres, "vertical-sides" => Surface::VerticalSides,
            _ => return Err(Error::Input("surface_model must be probe-centres or vertical-sides. A fixed-height rim alone does not measure wall slope.".into())),
        };
        let selected = read_selections(body)?;
        if selected.iter().any(|s| matches!(s.usage, Use::Face(..))) {
            return Err(Error::Input("Stock outline rows use fit, check or observe; named rectangular stock faces do not describe this contour.".into()));
        }
        if selected.iter().filter(|s| s.usage == Use::Fit).count()
            < if closure == Closure::Closed { 3 } else { 2 }
        {
            return Err(Error::Input("Select at least two ordered rim contacts for an open trace, or three for a closed trace. Keep separate check contacts.".into()));
        }
        let iterations = f["max_iterations"]
            .parse::<usize>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| {
                Error::Input("max_iterations needs a positive integer computational budget.".into())
            })?;
        Ok(Self {
            probe: Probe::read(&f)?,
            closure,
            surface,
            span: positive("neighborhood_span_mm")?,
            huber: positive("huber_mm")?,
            iterations,
            convergence: positive("convergence_mm")?,
            max_gap: positive("max_gap_mm")?,
            max_z_span: positive("max_z_span_mm")?,
            max_residual: positive("max_fit_residual_mm")?,
            selected,
        })
    }
}
