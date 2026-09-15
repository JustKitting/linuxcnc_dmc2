//! Reconstruct a measured surface while retaining unknown support and open edges.
mod extract;
mod field;
mod grid;
mod report;
pub(in crate::object_map) mod request;
#[cfg(test)]
mod tests;
use super::super::{folder, retained::Bundle};
use super::surface;
use crate::object_map::{
    model::Id,
    record,
    store::{read, save, Store},
    Error,
};
use std::path::Path;
pub fn prepare(store: &Store, object: &Id, setup: &Id, surface: &Id) -> Result<String, Error> {
    let source = Bundle::read(&folder(store, object, setup, surface)?)?;
    surface::request::Request::read(source.get("request.txt")?)?;
    let fields = request::KEYS
        .iter()
        .map(|k| {
            (
                *k,
                if *k == "surface_analysis" {
                    surface.as_str()
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
    let output = folder(store, object, setup, id)?;
    if output.exists() {
        return Err(Error::Storage("This stock-mesh analysis ID already exists. Choose a new ID to preserve its prior result.".into()));
    }
    let raw = read(input)?;
    let r = request::Request::read(&raw)?;
    let grid = grid::Grid::new(&r)?;
    let source = surface::load(store, object, setup, &r.surface)?;
    let field = field::run(
        &grid,
        &source.contacts,
        &source.stations,
        &source.request,
        &r,
    )?;
    let mesh = extract::run(&grid, &field.nodes, r.triangles, |v| {
        field.facet_support(v, &source.request, &r)
    })?;
    let report = report::build(&grid, &field.nodes, &mesh, &source.contacts, &r)?;
    save(&output.join("request.txt"), &raw)?;
    source.source.copy_to(&output, "surface-source-")?;
    for (name, bytes) in report.files {
        save(&output.join(name), bytes.as_bytes())?;
    }
    let manifest=format!("{{\"schema\":\"dmc2.stock-mesh-bundle.v1\",\"object\":{},\"setup\":{},\"analysis\":{},\"surface_analysis\":{},\"result\":{},\"cam_ready\":false}}\n",record::quote(object.as_str()),record::quote(setup.as_str()),record::quote(id.as_str()),record::quote(r.surface.as_str()),report.json);
    save(&output.join("manifest.json"), manifest.as_bytes())?;
    let (state, message) = report.outcome.description();
    Ok(format!(
        "{{\"analysis_directory\":{},\"state\":{},\"message\":{},\"cam_ready\":false}}",
        record::quote(&output.display().to_string()),
        record::quote(state),
        record::quote(message)
    ))
}
