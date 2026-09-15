//! Explicit acquisition history for novelty, independently of surface fit roles.
use super::{
    material,
    request::{Entry, History, Request, Use},
};
use crate::{
    object_map::{
        Error,
        model::{CaptureState, Id},
        positional::retained::Bundle,
        store::CaptureSnapshot,
    },
    probe_data::{
        ledger::number,
        mapper_schema::START_FIELDS,
        mapper_settings::{Phase, Sample, Settings, close},
        mapper_trace::{observation::TopColumn, state},
        schema::Workflow,
    },
};
use std::path::Path;

#[derive(Clone)]
pub struct Recorded {
    pub capture: Id,
    pub sample: Sample,
}
pub struct Source<'a> {
    pub settings: Settings,
    pub samples: Vec<Recorded>,
    pub captures: Vec<&'a CaptureSnapshot>,
}
fn find<'a>(captures: &'a [CaptureSnapshot], id: &Id) -> Result<&'a CaptureSnapshot, Error> {
    captures.iter().find(|c| c.id == *id).ok_or_else(|| Error::Data(format!("Capture {} is absent from this setup. Import its intact ledger and original companions before reusing the saved history.", id.as_str())))
}
fn acquisition(c: &CaptureSnapshot) -> Result<(Settings, Vec<Sample>), Error> {
    if c.capture.workflow != Workflow::Mapper {
        return Err(Error::Data(format!(
            "Capture {} is not a mapper run. Select an original mapper top capture with complete cycles.",
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
            "Capture {} is quarantined: {issues} Preserve this ledger for diagnosis and select a non-quarantined capture with complete original cycles.",
            c.id.as_str()
        )));
    }
    let settings = c.context.settings(&c.capture).map_err(Error::Data)?;
    TopColumn::new(&settings, [settings.origin[0], settings.origin[1]]).map_err(Error::Data)?;
    let samples = state::samples(&c.capture.records, &settings, false).map_err(|e| Error::Data(format!("Cannot use capture {} for new top samples: {e} Preserve its diagnosis and select an intact capture with complete cycles.", c.id.as_str())))?;
    if samples.is_empty() {
        return Err(Error::Data(format!(
            "Capture {} contains no complete top-search cycles. Preserve its original ledger and select a capture with retained cycles before planning.",
            c.id.as_str()
        )));
    }
    for sample in &samples {
        let q = sample.request;
        if !matches!(
            q.phase,
            Phase::Reference | Phase::Boundary | Phase::Grid | Phase::Verify
        ) || q.edge != -1
            || !close(q.target[0], q.approach[0])
            || !close(q.target[1], q.approach[1])
            || !close(q.target[2], settings.floor)
        {
            return Err(Error::Data(format!(
                "Capture {} record {} is not a retained vertical top-search column. Select a top capture whose original requests agree with its mode and descent floor; no side approach was reused.",
                c.id.as_str(),
                sample.sequence
            )));
        }
    }
    Ok((settings, samples))
}
fn primary<'a>(
    a: &material::Assessment,
    captures: &'a [CaptureSnapshot],
    id: &Id,
) -> Result<(&'a CaptureSnapshot, Settings), Error> {
    if !a.surface.contacts.iter().any(|s| s.capture == *id) {
        return Err(Error::Input("The selected top capture contributes no retained fine contacts to this material assessment. Select a contributing capture or calculate a new source surface/material analysis.".into()));
    }
    let c = find(captures, id)?;
    a.surface
        .source
        .require_equal(&format!("capture-{}.txt", id.as_str()), &c.raw)?;
    a.surface.source.check_context(c)?;
    let (settings, _) = acquisition(c)?;
    if !close(settings.radius, a.surface.request.probe.radius) {
        return Err(Error::Data("The retained acquisition ball radius disagrees with the surface analysis probe model. Resolve those original references before planning; no radius or travel envelope was substituted.".into()));
    }
    Ok((c, settings))
}
fn compatible(primary: &CaptureSnapshot, other: &CaptureSnapshot) -> Result<(), Error> {
    // Ignore mode only: original top and explicit follow-up modes can share
    // these acquisition settings. Numerical equality is not physical alignment.
    for key in START_FIELDS.iter().filter(|&&k| k != "mode") {
        let a = number(&primary.capture.records[0], key).map_err(Error::Data)?;
        let b = number(&other.capture.records[0], key).map_err(Error::Data)?;
        if !close(a, b) {
            return Err(Error::Data(format!(
                "Capture {} starting field {key}={b} differs from source {} ({a}). Exclude this capture or prepare a separate analysis in its original acquisition frame.",
                other.id.as_str(),
                primary.id.as_str()
            )));
        }
    }
    for (kind, bytes) in primary
        .context
        .snapshots()
        .filter(|(k, _)| matches!(*k, "plate" | "feeds"))
    {
        let other_bytes = other
            .context
            .snapshots()
            .find(|(k, _)| *k == kind)
            .and_then(|(_, b)| b);
        if bytes != other_bytes {
            return Err(Error::Data(format!(
                "Capture {} has a different original {kind} snapshot from source {}. Exclude it or prepare an analysis with matching original settings; current configuration cannot substitute.",
                other.id.as_str(),
                primary.id.as_str()
            )));
        }
    }
    Ok(())
}
pub fn prepare(
    a: &material::Assessment,
    captures: &[CaptureSnapshot],
    id: &Id,
) -> Result<Vec<Entry>, Error> {
    let (first, _) = primary(a, captures, id)?;
    Ok(captures.iter().map(|c| {
        let result = acquisition(c).and_then(|_| compatible(first, c));
        let (usage, reason) = match result {
            Ok(()) => (Use::Include, "Complete top cycles with matching recorded start and plate/feed snapshots; physical setup identity still requires review.".into()),
            Err(e) => (Use::Exclude, e.to_string()),
        };
        Entry { capture: c.id.clone(), usage, reason }
    }).collect())
}
pub fn load<'a>(
    a: &material::Assessment,
    captures: &'a [CaptureSnapshot],
    r: &Request,
) -> Result<Source<'a>, Error> {
    let (first, settings) = primary(a, captures, &r.capture)?;
    let ids = match &r.history {
        History::LegacySingleSource => vec![&r.capture],
        History::Explicit(entries) => entries
            .iter()
            .filter(|e| e.usage == Use::Include)
            .map(|e| &e.capture)
            .collect(),
    };
    let mut source = Source {
        settings,
        samples: Vec::new(),
        captures: Vec::new(),
    };
    for id in ids {
        let c = find(captures, id)?;
        let (_, samples) = acquisition(c)?;
        compatible(first, c)?;
        source
            .samples
            .extend(samples.into_iter().map(|sample| Recorded {
                capture: c.id.clone(),
                sample,
            }));
        source.captures.push(c);
    }
    Ok(source)
}
impl Source<'_> {
    pub fn retain(&self, output: &Path) -> Result<(), Error> {
        for c in &self.captures {
            c.export(&output.join(format!("history-capture-{}.txt", c.id.as_str())))?;
        }
        Ok(())
    }
    pub fn check_retained(&self, bundle: &Bundle) -> Result<(), Error> {
        for c in &self.captures {
            bundle.check_capture(c, "history-")?;
        }
        Ok(())
    }
}
