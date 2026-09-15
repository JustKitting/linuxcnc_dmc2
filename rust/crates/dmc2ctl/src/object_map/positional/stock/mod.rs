//! Acquisition-led stock estimation, reached through the standard offline binary.
mod fit;
mod report;
mod request;
#[cfg(test)]
mod tests;
use super::{
    super::{
        model::{CaptureState, Id, Stage},
        record,
        store::{read, save, Store},
        Error,
    },
    folder,
};
use crate::probe_data::{
    ledger::number,
    mapper_schema::Phase,
    mapper_settings::Mode,
    mapper_trace::{outline, state, Progress},
    schema::Workflow,
};
pub(in crate::object_map) use request::{KEYS, SCHEMA};
use std::{collections::BTreeSet, path::Path};

pub fn prepare(store: &Store, object: &Id, setup: &Id, capture: &Id) -> Result<String, Error> {
    let captures = store.captures(object, setup)?;
    let c = captures
        .iter()
        .find(|c| c.id == *capture)
        .ok_or_else(|| Error::Input("Select a capture retained in this setup.".into()))?;
    if c.capture.state == CaptureState::Quarantined {
        return Err(Error::Data("The selected capture is quarantined. Preserve its diagnosis and recapture required geometry after operator recovery.".into()));
    }
    let mut rows = String::from("capture,sequence,use\n");
    let mut selected = BTreeSet::new();
    if c.capture.workflow == Workflow::Mapper
        && Mode::read(number(&c.capture.records[0], "mode").map_err(Error::Data)?)
            .map_err(Error::Data)?
            == Mode::Outline
    {
        // Replay this run's retained policy, never today's configuration. A
        // coarse trial can precede its midpoint in time but follow it in space.
        let settings = c.context.settings(&c.capture).map_err(Error::Data)?;
        let samples = state::samples(&c.capture.records, &settings, false).map_err(Error::Data)?;
        let traced = outline::run(&settings, &samples);
        if let Err(Progress::Invalid(error)) = traced.result {
            return Err(Error::Data(error));
        }
        for sequence in traced.sequences {
            let phase =
                Phase::read(number(&c.capture.records[sequence], "phase").map_err(Error::Data)?)
                    .map_err(Error::Data)?;
            let usage = if phase == Phase::OutlineClose {
                "check"
            } else {
                "fit"
            };
            rows.push_str(&format!("{},{sequence},{usage}\n", c.id.as_str()));
            selected.insert(sequence);
        }
    }
    // Retain every other original fine contact as an observation. Trial points
    // and top samples are not silently deleted or assigned an invented order.
    for p in c
        .capture
        .contacts
        .iter()
        .filter(|p| p.stage == Stage::Fine && !selected.contains(&p.sequence))
    {
        rows.push_str(&format!("{},{},observe\n", c.id.as_str(), p.sequence));
    }
    let fields = KEYS
        .iter()
        .map(|key| (*key, "REQUIRED"))
        .collect::<Vec<_>>();
    String::from_utf8(record::encode(SCHEMA, &fields, rows.as_bytes())?)
        .map_err(|e| Error::Data(e.to_string()))
}
pub fn run(store: &Store, object: &Id, setup: &Id, id: &Id, path: &Path) -> Result<String, Error> {
    let output = folder(store, object, setup, id)?;
    if output.exists() {
        return Err(Error::Storage(format!(
            "Analysis {} already exists; use a new analysis ID.",
            output.display()
        )));
    }
    let raw = read(path)?;
    let req = request::Request::read(&raw)?;
    let captures = store.captures(object, setup)?;
    let samples = req.probe.samples(&captures, &req.selected)?;
    let contour = fit::run(&samples, &req)?;
    let report = report::build(&samples, &contour, &req)?;
    let used = samples
        .iter()
        .map(|s| s.capture.as_str())
        .collect::<BTreeSet<_>>();
    // Keep exact inputs and publish the manifest last, as for STL registration.
    save(&output.join("request.txt"), &raw)?;
    for c in &captures {
        if used.contains(c.id.as_str()) {
            c.export(&output.join(format!("capture-{}.txt", c.id.as_str())))?;
        }
    }
    save(&output.join("residuals.csv"), report.csv.as_bytes())?;
    save(
        &output.join("ball-centres.machine-mm.asc"),
        report.centers.as_bytes(),
    )?;
    save(
        &output.join("stock-outline.machine-mm.json"),
        report.json.as_bytes(),
    )?;
    save(
        &output.join("refinement-requests.json"),
        report.refinements.as_bytes(),
    )?;
    let sources = used
        .iter()
        .map(|x| record::quote(x))
        .collect::<Vec<_>>()
        .join(",");
    let manifest=format!("{{\"schema\":\"dmc2.stock-outline-bundle.v1\",\"object\":{},\"setup\":{},\"analysis\":{},\"captures\":[{}],\"result\":{},\"cam_ready\":false}}\n",record::quote(object.as_str()),record::quote(setup.as_str()),record::quote(id.as_str()),sources,report.json);
    save(&output.join("manifest.json"), manifest.as_bytes())?;
    Ok(format!("{{\"analysis_directory\":{},\"state\":\"unreviewed-stock-outline\",\"message\":\"Stock outline estimate retained with local residuals and measurement requests. Inspect the analysis and independent checks; material coverage and machining placement remain unresolved.\",\"cam_ready\":false}}",record::quote(&output.display().to_string())))
}
