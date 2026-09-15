//! Explicit capture decisions shared by surface support and acquisition history.
use super::{Error, model::Id, record::quote};
use std::collections::BTreeSet;
const HISTORY_HEADER: &str = "capture\tuse\treason";
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
impl Entry {
    pub fn json(&self) -> String {
        format!(
            "{{\"capture\":{},\"use\":{},\"request_annotation\":{}}}",
            quote(self.capture.as_str()),
            quote(self.usage.name()),
            quote(&self.reason)
        )
    }
}
pub fn read(payload: &[u8]) -> Result<Vec<Entry>, Error> {
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
    Ok(entries)
}
pub fn encode(entries: &[Entry]) -> String {
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
