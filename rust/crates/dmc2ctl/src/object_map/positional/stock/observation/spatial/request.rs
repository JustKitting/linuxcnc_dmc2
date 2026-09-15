pub use crate::object_map::capture_selection::{encode as history_text, Entry, Use};
use crate::object_map::{model::Id, positional::request::scalar, record, Error};
pub use crate::probe_data::top_followup::Role;

pub const SCHEMA: &str = "DMC2_SPATIAL_OBSERVATION_REQUEST_V1";
pub const HISTORY_SCHEMA: &str = "DMC2_SPATIAL_OBSERVATION_REQUEST_V2";
pub const ROLE_SCHEMA: &str = "DMC2_SPATIAL_OBSERVATION_REQUEST_V3";
pub fn recognizes(raw: &[u8]) -> bool {
    [SCHEMA, HISTORY_SCHEMA, ROLE_SCHEMA]
        .iter()
        .any(|s| raw.starts_with(format!("{s}\n").as_bytes()))
}
pub enum History {
    LegacySingleSource,
    Explicit(Vec<Entry>),
}
impl History {
    fn read(payload: &[u8], primary: &Id) -> Result<Self, Error> {
        let entries = crate::object_map::capture_selection::read(payload)?;
        if !entries
            .iter()
            .any(|e| e.capture == *primary && e.usage == Use::Include)
        {
            return Err(Error::Input("The selected source capture must be included in the explicit acquisition history. Include it or prepare a request from the intended source.".into()));
        }
        Ok(Self::Explicit(entries))
    }
}
pub const KEYS: &[&str] = &[
    "material_analysis",
    "source_capture",
    "sample_spacing_mm",
    "max_observations",
    "max_grid_cells",
    "max_candidate_comparisons",
];
pub fn role_keys() -> Vec<&'static str> {
    KEYS.iter().copied().chain(["contact_role"]).collect()
}
pub struct Request {
    pub role: Option<Role>,
    pub material: Id,
    pub capture: Id,
    pub spacing: f64,
    pub observations: usize,
    pub cells: usize,
    pub comparisons: usize,
    pub history: History,
}
impl Request {
    pub fn read(raw: &[u8]) -> Result<Self, Error> {
        let schema = if raw.starts_with(format!("{ROLE_SCHEMA}\n").as_bytes()) {
            ROLE_SCHEMA
        } else if raw.starts_with(format!("{HISTORY_SCHEMA}\n").as_bytes()) {
            HISTORY_SCHEMA
        } else {
            SCHEMA
        };
        let keys = if schema == ROLE_SCHEMA {
            role_keys()
        } else {
            KEYS.to_vec()
        };
        let (f, payload) = record::decode(raw, schema, &keys)?;
        let role = if schema == ROLE_SCHEMA {
            Some(Role::read(&f["contact_role"]).map_err(Error::Input)?)
        } else {
            None
        };
        if schema == SCHEMA && !payload.is_empty() {
            return Err(Error::Input("New top samples are derived from the retained material assessment. Remove the extra target payload and edit the explicit sampling settings.".into()));
        }
        let spacing = scalar(&f["sample_spacing_mm"], "sample_spacing_mm")?;
        if spacing <= 0. {
            return Err(Error::Input("sample_spacing_mm must be positive. Choose the spatial sampling interval; no spacing or descent distance is guessed.".into()));
        }
        let budget = |key: &str| {
            f[key].parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(|| Error::Input(format!("{key} needs a positive integer budget. Edit the request and retry the analysis; no acquisition bounds are extended.")))
        };
        let capture = Id::parse(&f["source_capture"])?;
        let history = if schema != SCHEMA {
            History::read(payload, &capture)?
        } else {
            History::LegacySingleSource
        };
        Ok(Self {
            role,
            material: Id::parse(&f["material_analysis"])?,
            capture,
            spacing,
            observations: budget("max_observations")?,
            cells: budget("max_grid_cells")?,
            comparisons: budget("max_candidate_comparisons")?,
            history,
        })
    }
}
