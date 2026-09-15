//! Material-directed observation selection with original acquisition bounds.
mod report;
pub(in crate::object_map) mod request;
mod select;
pub(in crate::object_map) mod spatial;
#[cfg(test)]
mod tests;
use super::super::retained::Bundle;
use super::{material, surface};
use crate::object_map::{
    Error,
    model::Id,
    record,
    store::{Store, read, save},
};
use std::path::Path;

pub fn prepare(store: &Store, object: &Id, setup: &Id, material: &Id) -> Result<String, Error> {
    material::load(store, object, setup, material)?;
    let fields = request::KEYS
        .iter()
        .map(|k| {
            (
                *k,
                if *k == "material_analysis" {
                    material.as_str()
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
        return Err(Error::Storage("This observation analysis already exists. Select a new ID to preserve its proposals and original sources.".into()));
    }
    let raw = read(input)?;
    if spatial::request::recognizes(&raw) {
        return spatial::run(store, object, setup, id, &output, &raw);
    }
    let r = request::Request::read(&raw)?;
    let (source, a) = material::load(store, object, setup, &r.material)?;
    let captures = store.captures(object, setup)?;
    let selection = select::run(&a, &captures, &r)?;
    let report = report::build(&a, &selection)?;
    let manifest = format!(
        "{{\"schema\":\"dmc2.observation-plan-bundle.v1\",\"object\":{},\"setup\":{},\"analysis\":{},\"material_analysis\":{},\"result\":{},\"cam_ready\":false,\"machine_action_authorized\":false}}\n",
        record::quote(object.as_str()),
        record::quote(setup.as_str()),
        record::quote(id.as_str()),
        record::quote(r.material.as_str()),
        report.json
    );
    retain(&output, &raw, &source, &report, &manifest)?;
    Ok(format!(
        "{{\"analysis_directory\":{},\"state\":\"unreviewed-observation-proposals\",\"selected_observations\":{},\"unplanned_patch_requirements\":{},\"message\":\"Inspect the selected original approaches, entry prerequisites and retained unresolved regions. Fresh captures and an approved entry/execution path are still required; no old contact was promoted to a new independent check.\",\"cam_ready\":false,\"machine_action_authorized\":false}}",
        record::quote(&output.display().to_string()),
        selection.chosen.len(),
        selection.pending.len()
    ))
}

fn retain(
    output: &Path,
    raw: &[u8],
    source: &Bundle,
    report: &report::Report,
    manifest: &str,
) -> Result<(), Error> {
    save(&output.join("request.txt"), raw)?;
    source.copy_to(output, "material-source-")?;
    save(
        &output.join("observation-plan.machine-mm.json"),
        report.json.as_bytes(),
    )?;
    save(&output.join("residuals.csv"), report.csv.as_bytes())?;
    save(&output.join("manifest.json"), manifest.as_bytes())
}
