use crate::object_map::{Error, model::Id, record};
pub const SCHEMA: &str = "DMC2_OBSERVATION_REQUEST_V1";
pub const KEYS: &[&str] = &[
    "material_analysis",
    "max_observations",
    "max_candidate_need_comparisons",
];
pub struct Request {
    pub material: Id,
    pub observations: usize,
    pub comparisons: usize,
}
impl Request {
    pub fn read(raw: &[u8]) -> Result<Self, Error> {
        let (f, payload) = record::decode(raw, SCHEMA, KEYS)?;
        if !payload.is_empty() {
            return Err(Error::Input("Observation selection uses the retained material analysis. Remove the extra payload; no manually substituted probe targets are accepted here.".into()));
        }
        let budget = |key: &str| {
            f[key].parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(|| Error::Input(format!("{key} needs a positive integer budget. Edit the observation request; no acquisition range or speed is inferred.")))
        };
        Ok(Self {
            material: Id::parse(&f["material_analysis"])?,
            observations: budget("max_observations")?,
            comparisons: budget("max_candidate_need_comparisons")?,
        })
    }
}
