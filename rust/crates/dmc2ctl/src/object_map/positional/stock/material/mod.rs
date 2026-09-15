//! Candidate-directed local material assessment, outside the control process.
mod empty;
pub(super) mod query;
mod report;
pub(in crate::object_map) mod request;
#[cfg(test)]
mod tests;
use super::super::{cover, retained::Bundle};
use super::{candidate::Candidate, surface};
use crate::object_map::{
    model::Id,
    record,
    store::{read, save, Store},
    Error,
};
use std::path::Path;

pub(super) struct Assessment {
    pub request: request::Request,
    pub candidate: Candidate,
    pub surface: surface::Loaded,
    pub covers: Vec<cover::Sample>,
    pub regions: Vec<query::Region>,
}
fn assess(store: &Store, object: &Id, setup: &Id, raw: &[u8]) -> Result<Assessment, Error> {
    let request = request::Request::read(raw)?;
    let candidate = Candidate::load(store, object, setup, &request.candidate)?;
    let surface = surface::load(store, object, setup, &request.surface)?;
    candidate.require_frame(&surface.source)?;
    let covers = cover::build(
        candidate.mesh.triangles(),
        request.radius,
        request.samples,
        cover::Metric::Spatial,
    )?;
    let regions = query::run(
        &covers,
        &surface.contacts,
        &surface.stations,
        &surface.request,
        candidate.pose,
        &request,
        &surface.no_contact,
    )?;
    Ok(Assessment {
        request,
        candidate,
        surface,
        covers,
        regions,
    })
}
fn report(a: &Assessment) -> Result<report::Report, Error> {
    report::build(
        &a.covers,
        &a.surface.contacts,
        &a.regions,
        a.candidate.mesh.triangles(),
        a.candidate.pose,
        &a.request,
        &a.surface.no_contact,
    )
}
fn manifest(object: &Id, setup: &Id, id: &Id, a: &Assessment, json: &str) -> String {
    format!(
        "{{\"schema\":\"dmc2.material-check-bundle.{}\",\"object\":{},\"setup\":{},\"analysis\":{},\"candidate_analysis\":{},\"surface_analysis\":{},\"result\":{},\"cam_ready\":false}}\n",
        a.request.empty.version(),
        record::quote(object.as_str()),
        record::quote(setup.as_str()),
        record::quote(id.as_str()),
        record::quote(a.request.candidate.as_str()),
        record::quote(a.request.surface.as_str()),
        json
    )
}
pub(super) fn load(
    store: &Store,
    object: &Id,
    setup: &Id,
    id: &Id,
) -> Result<(Bundle, Assessment), Error> {
    let source = Bundle::read(&super::super::folder(store, object, setup, id)?)?;
    let a = assess(store, object, setup, source.get("request.txt")?)?;
    source.require_source(&a.candidate.bundle, "candidate-source-")?;
    source.require_source(&a.surface.source, "surface-source-")?;
    let r = report(&a)?;
    for (name, text) in [
        ("residuals.csv", &r.csv),
        ("measurement-needs.json", &r.needs),
        ("material-check.machine-mm.json", &r.json),
    ] {
        source.require_equal(name, text.as_bytes())?;
    }
    source.require_equal(
        "manifest.json",
        manifest(object, setup, id, &a, &r.json).as_bytes(),
    )?;
    Ok((source, a))
}

pub fn prepare(store: &Store, object: &Id, setup: &Id, candidate: &Id) -> Result<String, Error> {
    Candidate::load(store, object, setup, candidate)?;
    let keys = request::keys();
    let fields = keys
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
    let a = assess(store, object, setup, &raw)?;
    let report = report(&a)?;
    // Original candidate, source analysis and exact triggers survive together.
    save(&output.join("request.txt"), &raw)?;
    a.candidate.bundle.copy_to(&output, "candidate-source-")?;
    a.surface.source.copy_to(&output, "surface-source-")?;
    save(&output.join("residuals.csv"), report.csv.as_bytes())?;
    save(
        &output.join("measurement-needs.json"),
        report.needs.as_bytes(),
    )?;
    save(
        &output.join("material-check.machine-mm.json"),
        report.json.as_bytes(),
    )?;
    let manifest = manifest(object, setup, id, &a, &report.json);
    save(&output.join("manifest.json"), manifest.as_bytes())?;
    Ok(format!(
        "{{\"analysis_directory\":{},\"state\":\"material-coverage-unresolved\",\"message\":\"Local material comparisons and candidate-directed measurement regions are retained. Inspect each shortage, missing support and independent check. Closed volume and cutting remain unresolved.\",\"cam_ready\":false}}",
        record::quote(&output.display().to_string())
    ))
}
