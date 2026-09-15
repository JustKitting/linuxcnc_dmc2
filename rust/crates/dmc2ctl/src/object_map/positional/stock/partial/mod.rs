//! Explicitly requested placement refinement from a retained material assessment.
mod report;
pub(in crate::object_map) mod request;
mod search;
use super::material;
use crate::object_map::{
    model::Id,
    positional::{folder, pose_record},
    record,
    store::{read, save, Store},
    Error,
};
use std::path::Path;

pub fn prepare(store: &Store, object: &Id, setup: &Id, source: &Id) -> Result<String, Error> {
    let (_, a) = material::load(store, object, setup, source)?;
    if !matches!(
        a.request.empty,
        material::request::EmptySpace::RequiredVolume { .. }
    ) {
        return Err(Error::Input("Prepare a V3 material check with the required-material boundary and retained no-contact model before partial placement refinement.".into()));
    }
    let units = a.candidate.units.to_string();
    let fields = request::KEYS
        .iter()
        .map(|k| {
            (
                *k,
                match *k {
                    "material_analysis" => source.as_str(),
                    "design" => a.candidate.design.as_str(),
                    "stl_mm_per_unit" => &units,
                    _ => "REQUIRED",
                },
            )
        })
        .collect::<Vec<_>>();
    String::from_utf8(record::encode(request::SCHEMA, &fields, &[])?)
        .map_err(|e| Error::Data(e.to_string()))
}
pub fn run(store: &Store, object: &Id, setup: &Id, id: &Id, input: &Path) -> Result<String, Error> {
    let output = folder(store, object, setup, id)?;
    if output.exists() {
        return Err(Error::Storage("This partial-placement analysis ID exists. Select a new ID to preserve the previous placement.".into()));
    }
    let raw = read(input)?;
    let r = request::Request::read(&raw)?;
    let (source, mut a) = material::load(store, object, setup, &r.material)?;
    if r.design != a.candidate.design || r.units != a.candidate.units {
        return Err(Error::Input("Partial refinement must retain the selected material assessment's original design and unit conversion. Restore those fields; attach a different design as a separate revision and assessment.".into()));
    }
    let original_pose = a.candidate.pose;
    let (fitted, result) = {
        let objective = search::ObjectiveData::new(&a, &r)?;
        let f = objective.run(&r)?;
        let before = objective.rows([0.; 6])?;
        let after = objective.rows(f.at)?;
        let result = report::build(&a, &objective, &f, &before, &after);
        (f, result)
    };
    let pose = search::pose(original_pose, fitted.at).validate()?;
    material::reposition(&mut a, pose)?;
    // The rechecked report describes THIS new candidate. It does not overwrite
    // its source material assessment or reinterpret that source's placement.
    a.request.candidate = id.clone();
    let checked = material::report(&a)?;
    let decision = material::Decision::from(&a);
    let transformed = a.candidate.mesh.transformed_stl(pose)?;
    let (mut fields, payload) = record::decode(
        source.get("request.txt")?,
        material::request::SCHEMA,
        &material::request::keys(),
    )?;
    fields.insert("candidate_analysis".into(), id.as_str().into());
    let fields = fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect::<Vec<_>>();
    let check_request = record::encode(material::request::SCHEMA, &fields, payload)?;
    // Copy the actual acquisition source once, avoiding recursively nested
    // candidate bundles as the operator repeats refinement/measurement cycles.
    save(&output.join("request.txt"), &raw)?;
    save(
        &output.join("source.stl"),
        a.candidate.bundle.get("source.stl")?,
    )?;
    a.surface.source.copy_to(&output, "surface-source-")?;
    save(
        &output.join("material-source-request.txt"),
        source.get("request.txt")?,
    )?;
    for (old, new) in [
        (
            "material-check.machine-mm.json",
            "initial-material-check.machine-mm.json",
        ),
        ("measurement-needs.json", "initial-measurement-needs.json"),
        ("residuals.csv", "initial-material-residuals.csv"),
    ] {
        save(&output.join(new), source.get(old)?)?;
    }
    save(
        &output.join("initial-pose.txt"),
        &pose_record(original_pose)?,
    )?;
    save(&output.join("pose-candidate.txt"), &pose_record(pose)?)?;
    save(
        &output.join("model-candidate.machine-mm.stl"),
        transformed.as_bytes(),
    )?;
    save(&output.join("residuals.csv"), result.csv.as_bytes())?;
    save(
        &output.join("search-history.csv"),
        result.history.as_bytes(),
    )?;
    save(
        &output.join("material-check.machine-mm.json"),
        checked.json.as_bytes(),
    )?;
    save(
        &output.join("material-residuals.csv"),
        checked.csv.as_bytes(),
    )?;
    save(
        &output.join("measurement-needs.json"),
        checked.needs.as_bytes(),
    )?;
    save(&output.join("material-check-request.txt"), &check_request)?;
    save(
        &output.join("pipeline-state.json"),
        decision.json().as_bytes(),
    )?;
    let manifest=format!("{{\"schema\":\"dmc2.partial-placement-bundle.v1\",\"object\":{},\"setup\":{},\"analysis\":{},\"material_analysis\":{},\"surface_analysis\":{},\"design\":{},\"result\":{},\"pipeline\":{},\"cam_ready\":false}}\n",record::quote(object.as_str()),record::quote(setup.as_str()),record::quote(id.as_str()),record::quote(r.material.as_str()),record::quote(a.request.surface.as_str()),record::quote(r.design.as_str()),result.json,decision.json());
    save(&output.join("manifest.json"), manifest.as_bytes())?;
    decision.result(&output)
}
