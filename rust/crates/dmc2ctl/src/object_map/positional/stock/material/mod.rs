//! Candidate-directed local material assessment, outside the control process.
mod query;
mod report;
pub(in crate::object_map) mod request;
#[cfg(test)]
mod tests;
use super::super::cover;
use super::{candidate::Candidate, surface};
use crate::object_map::{
    Error,
    model::Id,
    record,
    store::{Store, read, save},
};
use std::path::Path;

pub fn prepare(store: &Store, object: &Id, setup: &Id, candidate: &Id) -> Result<String, Error> {
    Candidate::load(store, object, setup, candidate)?;
    let fields = request::KEYS
        .iter()
        .map(|k| {
            (
                *k,
                if *k == "candidate_analysis" {
                    candidate.as_str()
                } else {
                    "REQUIRED"
                },
            )
        })
        .collect::<Vec<_>>();
    String::from_utf8(record::encode(request::SCHEMA, &fields, &[])?)
        .map_err(|e| Error::Data(e.to_string()))
}
pub fn run(store: &Store, object: &Id, setup: &Id, id: &Id, input: &Path) -> Result<String, Error> {
    let output = super::super::folder(store, object, setup, id)?;
    if output.exists() {
        return Err(Error::Storage("This material analysis ID already exists. Select a new ID to preserve the prior result.".into()));
    }
    let raw = read(input)?;
    let r = request::Request::read(&raw)?;
    let candidate = Candidate::load(store, object, setup, &r.candidate)?;
    let pose = candidate.pose;
    let mesh = &candidate.mesh;
    let loaded = surface::load(store, object, setup, &r.surface)?;
    let source = &loaded.source;
    let sr = &loaded.request;
    let contacts = &loaded.contacts;
    let stations = &loaded.stations;
    candidate.require_frame(source)?;
    let samples = cover::build(
        mesh.triangles(),
        r.radius,
        r.samples,
        cover::Metric::Spatial,
    )?;
    let result = query::run(&samples, contacts, stations, sr, pose, &r)?;
    let report = report::build(&samples, contacts, &result, mesh.triangles(), pose, &r)?;
    // Original candidate, source analysis and exact triggers survive together.
    save(&output.join("request.txt"), &raw)?;
    candidate.bundle.copy_to(&output, "candidate-source-")?;
    source.copy_to(&output, "surface-source-")?;
    save(&output.join("residuals.csv"), report.csv.as_bytes())?;
    save(
        &output.join("measurement-needs.json"),
        report.needs.as_bytes(),
    )?;
    save(
        &output.join("material-check.machine-mm.json"),
        report.json.as_bytes(),
    )?;
    let manifest = format!(
        "{{\"schema\":\"dmc2.material-check-bundle.v1\",\"object\":{},\"setup\":{},\"analysis\":{},\"candidate_analysis\":{},\"surface_analysis\":{},\"result\":{},\"cam_ready\":false}}\n",
        record::quote(object.as_str()),
        record::quote(setup.as_str()),
        record::quote(id.as_str()),
        record::quote(r.candidate.as_str()),
        record::quote(r.surface.as_str()),
        report.json
    );
    save(&output.join("manifest.json"), manifest.as_bytes())?;
    Ok(format!(
        "{{\"analysis_directory\":{},\"state\":\"material-coverage-unresolved\",\"message\":\"Local material comparisons and candidate-directed measurement regions are retained. Inspect each shortage, missing support and independent check. Closed volume and cutting remain unresolved.\",\"cam_ready\":false}}",
        record::quote(&output.display().to_string())
    ))
}
