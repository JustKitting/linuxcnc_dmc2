use crate::object_map::{Error, model::Id, positional::request::scalar, record};

pub const SCHEMA: &str = "DMC2_SPATIAL_OBSERVATION_REQUEST_V1";
pub const KEYS: &[&str] = &[
    "material_analysis",
    "source_capture",
    "sample_spacing_mm",
    "max_observations",
    "max_grid_cells",
    "max_candidate_comparisons",
];
pub struct Request {
    pub material: Id,
    pub capture: Id,
    pub spacing: f64,
    pub observations: usize,
    pub cells: usize,
    pub comparisons: usize,
}
impl Request {
    pub fn read(raw: &[u8]) -> Result<Self, Error> {
        let (f, payload) = record::decode(raw, SCHEMA, KEYS)?;
        if !payload.is_empty() {
            return Err(Error::Input("New top samples are derived from the retained material assessment. Remove the extra target payload and edit the explicit sampling settings.".into()));
        }
        let spacing = scalar(&f["sample_spacing_mm"], "sample_spacing_mm")?;
        if spacing <= 0. {
            return Err(Error::Input("sample_spacing_mm must be positive. Choose the spatial sampling interval; no spacing or descent distance is guessed.".into()));
        }
        let budget = |key: &str| {
            f[key].parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(|| Error::Input(format!("{key} needs a positive integer budget. Edit the request and retry the analysis; no acquisition bounds are extended.")))
        };
        Ok(Self {
            material: Id::parse(&f["material_analysis"])?,
            capture: Id::parse(&f["source_capture"])?,
            spacing,
            observations: budget("max_observations")?,
            cells: budget("max_grid_cells")?,
            comparisons: budget("max_candidate_comparisons")?,
        })
    }
}
