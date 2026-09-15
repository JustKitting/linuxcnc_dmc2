use crate::object_map::{Error, model::Id, positional::request::scalar, record};
use std::collections::BTreeSet;

pub const SCHEMA: &str = "DMC2_SPATIAL_OBSERVATION_REQUEST_V1";
pub const HISTORY_SCHEMA: &str = "DMC2_SPATIAL_OBSERVATION_REQUEST_V2";
const HISTORY_HEADER: &str = "capture\tuse\treason";
pub fn recognizes(raw: &[u8]) -> bool {
    [SCHEMA, HISTORY_SCHEMA]
        .iter()
        .any(|s| raw.starts_with(format!("{s}\n").as_bytes()))
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Use {
    Include,
    Exclude,
}
impl Use {
    pub fn name(self) -> &'static str {
        match self {
            Self::Include => "include",
            Self::Exclude => "exclude",
        }
    }
}
pub struct Entry {
    pub capture: Id,
    pub usage: Use,
    pub reason: String,
}
pub enum History {
    LegacySingleSource,
    Explicit(Vec<Entry>),
}
impl History {
    fn read(payload: &[u8], primary: &Id) -> Result<Self, Error> {
        let text = std::str::from_utf8(payload).map_err(|e| Error::Input(format!("Acquisition history is not UTF-8: {e}. Prepare a new request with its original history list.")))?;
        let mut lines = text.lines();
        if lines.next() != Some(HISTORY_HEADER) {
            return Err(Error::Input("Acquisition history needs the tab-separated capture/use/reason header. Prepare a new request; do not substitute current captures for a saved history.".into()));
        }
        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();
        for line in lines {
            let row = line.split('\t').collect::<Vec<_>>();
            if row.len() != 3 || row[2].trim().is_empty() || row[2].chars().any(char::is_control) {
                return Err(Error::Input("Each acquisition-history row needs a capture ID, include/exclude and a nonempty reason separated by tabs. Correct the request and retry.".into()));
            }
            let capture = Id::parse(row[0])?;
            if !seen.insert(capture.as_str().to_owned()) {
                return Err(Error::Input(format!(
                    "Capture {} occurs more than once in the history. Keep one explicit decision per capture and retry.",
                    capture.as_str()
                )));
            }
            let usage = match row[1] {
                "include" => Use::Include,
                "exclude" => Use::Exclude,
                _ => return Err(Error::Input("Acquisition history use must be include or exclude. Correct the decision and retry.".into())),
            };
            entries.push(Entry {
                capture,
                usage,
                reason: row[2].into(),
            });
        }
        if !entries
            .iter()
            .any(|e| e.capture == *primary && e.usage == Use::Include)
        {
            return Err(Error::Input("The selected source capture must be included in the explicit acquisition history. Include it or prepare a request from the intended source.".into()));
        }
        Ok(Self::Explicit(entries))
    }
}
pub fn history_text(entries: &[Entry]) -> String {
    let mut text = format!("{HISTORY_HEADER}\n");
    for e in entries {
        // Diagnostics are retained as readable single-line annotations.
        let reason = e
            .reason
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>();
        text.push_str(&format!(
            "{}\t{}\t{reason}\n",
            e.capture.as_str(),
            e.usage.name()
        ));
    }
    text
}
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
    pub history: History,
}
impl Request {
    pub fn read(raw: &[u8]) -> Result<Self, Error> {
        let schema = if raw.starts_with(format!("{HISTORY_SCHEMA}\n").as_bytes()) {
            HISTORY_SCHEMA
        } else {
            SCHEMA
        };
        let (f, payload) = record::decode(raw, schema, KEYS)?;
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
        let history = if schema == HISTORY_SCHEMA {
            History::read(payload, &capture)?
        } else {
            History::LegacySingleSource
        };
        Ok(Self {
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
