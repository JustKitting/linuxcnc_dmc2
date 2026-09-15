//! Retained coarse-miss paths, before applying the declared probe/error model.
use super::{V, add, finite};
use crate::{
    object_map::{Error, model::CaptureState, store::CaptureSnapshot},
    probe_data::{
        ledger::number,
        mapper_settings::{close, xyz},
        mapper_trace::state,
        schema::Workflow,
    },
};

pub struct Path {
    pub sequence: usize,
    pub from: V,
    pub reported_end: V,
    pub requested_end: V,
    pub feed: f64,
}
pub fn read(c: &CaptureSnapshot) -> Result<Vec<Path>, Error> {
    if c.capture.workflow != Workflow::Mapper {
        return Err(Error::Data(format!(
            "Capture {} is not a mapper run. Exclude it from no-contact sources and select original mapper coarse-miss cycles; no empty-space constraint was inferred.",
            c.id.as_str()
        )));
    }
    if c.capture.state == CaptureState::Quarantined {
        let issues = c
            .capture
            .issues
            .iter()
            .map(|i| format!("record {}: {}", i.sequence, i.kind.message()))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(Error::Data(format!(
            "Capture {} is quarantined: {issues} Preserve its diagnosis and exclude it from no-contact sources before retrying the analysis.",
            c.id.as_str()
        )));
    }
    if !c.capture.records.iter().any(|r| r["kind"] == "miss") {
        return Err(Error::Input(format!(
            "Capture {} contains no original coarse-miss records. Exclude it from no-contact sources; its fine contacts can still be selected separately.",
            c.id.as_str()
        )));
    }
    let s = c.context.settings(&c.capture).map_err(Error::Data)?;
    let retained = state::samples(&c.capture.records, &s, false).map_err(|e| Error::Data(format!("Cannot use capture {} for no-contact support: {e} Preserve the ledger and exclude it from this analysis until its original capture context is resolved.",c.id.as_str())))?;
    let mut paths = Vec::new();
    for record in c.capture.records.iter().filter(|r| r["kind"] == "miss") {
        let sequence = record["sequence"].parse::<usize>().map_err(|_| Error::Data("A no-contact record has an invalid identity. Preserve the source and import an intact capture.".into()))?;
        let sample = retained.iter().find(|sample| sample.sequence == sequence && sample.trigger.is_none())
            .ok_or_else(|| Error::Data(format!("{}:{sequence} is not a retained complete coarse-miss cycle. Preserve its diagnosis and exclude this capture from the analysis until recaptured after operator recovery.",c.id.as_str())))?;
        let from = xyz(record, "from_", "").map_err(Error::Data)?;
        let end = xyz(record, "work_", "").map_err(Error::Data)?;
        let feed = number(record, "feed").map_err(Error::Data)?;
        if !s.endpoint_matches(end, sample.request.target)
            || !close(feed, s.coarse_feed(sample.request.phase))
        {
            return Err(Error::Data(format!(
                "{}:{sequence} lacks a reported endpoint and feed matching its retained coarse search. No full-travel no-contact constraint was accepted; preserve and inspect this capture.",
                c.id.as_str()
            )));
        }
        s.bounds(from).map_err(Error::Data)?;
        s.bounds(end).map_err(Error::Data)?;
        let from = add(from, s.offset);
        let reported_end = add(end, s.offset);
        let requested_end = add(sample.request.target, s.offset);
        if ![from, reported_end, requested_end].into_iter().all(finite) {
            return Err(Error::Data(format!(
                "{}:{sequence} has unrepresentable machine coordinates. Inspect original units and work offsets before reusing it.",
                c.id.as_str()
            )));
        }
        paths.push(Path {
            sequence,
            from,
            reported_end,
            requested_end,
            feed,
        });
    }
    Ok(paths)
}
