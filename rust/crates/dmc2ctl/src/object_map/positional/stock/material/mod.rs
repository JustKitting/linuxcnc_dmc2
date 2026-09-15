//! Candidate-directed local material assessment, outside the control process.
mod query;
mod report;
pub(in crate::object_map) mod request;
#[cfg(test)]
mod tests;
use super::super::{cover, mesh::Mesh, read_pose, retained::Bundle};
use super::{placement, surface};
use crate::object_map::{
    model::Id,
    record,
    store::{read, save, Store},
    Error,
};
use std::path::Path;

pub fn prepare(store: &Store, object: &Id, setup: &Id, candidate: &Id) -> Result<String, Error> {
    let dir = super::super::folder(store, object, setup, candidate)?;
    let source = Bundle::read(&dir)?;
    placement::request::Request::read(source.get("request.txt")?)?;
    read_pose(&dir.join("pose-candidate.txt"))?;
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
    let dir = super::super::folder(store, object, setup, &r.candidate)?;
    let candidate = Bundle::read(&dir)?;
    let placement = placement::request::Request::read(candidate.get("request.txt")?)?;
    let pose = read_pose(&dir.join("pose-candidate.txt"))?;
    let mesh = Mesh::read(candidate.get("source.stl")?, placement.units)?;
    mesh.fitting_geometry()?;
    let transformed = mesh.transformed_stl(pose)?;
    candidate.require_equal("model-candidate.machine-mm.stl", transformed.as_bytes())?;
    let source = Bundle::read(&super::super::folder(store, object, setup, &r.surface)?)?;
    let sr = surface::request::Request::read(source.get("request.txt")?)?;
    let (outline_fields, _) = record::decode(
        candidate.get("stock-source-request.txt")?,
        super::request::SCHEMA,
        &super::request::keys(),
    )?;
    let (surface_fields, _) = record::decode(
        source.get("request.txt")?,
        surface::request::SCHEMA,
        &surface::request::keys(),
    )?;
    if outline_fields["frame_reference"] != surface_fields["frame_reference"] {
        return Err(Error::Data("The candidate outline and 3D surfaces declare different frame references. Resolve their actual setup relationship and retain matching source analyses before comparison; no implicit registration was applied.".into()));
    }
    let captures = store.captures(object, setup)?;
    let contacts = sr.probe.samples(&captures, &sr.selected)?;
    source.check_captures(&captures, &contacts)?;
    let stations = surface::fit::run(&contacts, &sr);
    let surface_report = surface::report::build(&contacts, &stations, &sr)?;
    source.require_equal(
        "stock-surface.machine-mm.json",
        surface_report.json.as_bytes(),
    )?;
    let samples = cover::build(
        mesh.triangles(),
        r.radius,
        r.samples,
        cover::Metric::Spatial,
    )?;
    let result = query::run(&samples, &contacts, &stations, &sr, pose, &r)?;
    let report = report::build(&samples, &contacts, &result, mesh.triangles(), pose, &r)?;
    // Original candidate, source analysis and exact triggers survive together.
    save(&output.join("request.txt"), &raw)?;
    candidate.copy_to(&output, "candidate-source-")?;
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
    let manifest = format!("{{\"schema\":\"dmc2.material-check-bundle.v1\",\"object\":{},\"setup\":{},\"analysis\":{},\"candidate_analysis\":{},\"surface_analysis\":{},\"result\":{},\"cam_ready\":false}}\n",record::quote(object.as_str()),record::quote(setup.as_str()),record::quote(id.as_str()),record::quote(r.candidate.as_str()),record::quote(r.surface.as_str()),report.json);
    save(&output.join("manifest.json"), manifest.as_bytes())?;
    Ok(format!("{{\"analysis_directory\":{},\"state\":\"material-coverage-unresolved\",\"message\":\"Local material comparisons and candidate-directed measurement regions are retained. Inspect each shortage, missing support and independent check. Closed volume and cutting remain unresolved.\",\"cam_ready\":false}}",record::quote(&output.display().to_string())))
}
