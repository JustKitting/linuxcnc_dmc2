//! New material-directed top columns through the normal Object Mapper path.
pub(in crate::object_map) mod export;
mod grid;
mod report;
pub(in crate::object_map) mod request;
mod select;
#[cfg(test)]
mod tests;
use super::material;
use crate::object_map::{Error, model::Id, record, store::Store};
use source::Source;
use std::path::Path;
mod source;

pub fn prepare(
    store: &Store,
    object: &Id,
    setup: &Id,
    material: &Id,
    capture: &Id,
) -> Result<String, Error> {
    let (_, a) = material::load(store, object, setup, material)?;
    let history = source::prepare(&a, &store.captures(object, setup)?, capture)?;
    let fields = request::KEYS
        .iter()
        .map(|k| {
            (
                *k,
                match *k {
                    "material_analysis" => material.as_str(),
                    "source_capture" => capture.as_str(),
                    _ => "REQUIRED",
                },
            )
        })
        .collect::<Vec<_>>();
    String::from_utf8(record::encode(
        request::HISTORY_SCHEMA,
        &fields,
        request::history_text(&history).as_bytes(),
    )?)
    .map_err(|e| Error::Data(e.to_string()))
}
pub(super) fn run(
    store: &Store,
    object: &Id,
    setup: &Id,
    id: &Id,
    output: &Path,
    raw: &[u8],
) -> Result<String, Error> {
    let r = request::Request::read(raw)?;
    let (bundle, a) = material::load(store, object, setup, &r.material)?;
    let captures = store.captures(object, setup)?;
    let source = source::load(&a, &captures, &r)?;
    let selection = select::run(&a, &source, &r)?;
    let report = report::build(&a, &source, &selection, &r)?;
    let manifest = manifest(object, setup, id, &r, &report.json);
    if matches!(&r.history, request::History::Explicit(_)) {
        source.retain(output)?;
    }
    super::retain(output, raw, &bundle, &report, &manifest)?;
    Ok(format!(
        "{{\"analysis_directory\":{},\"state\":\"unreviewed-spatial-observation-proposals\",\"spatial_cells\":{},\"selected_observations\":{},\"unsupported_material_regions\":{},\"message\":\"New top-column proposals and all unresolved material regions are retained. Inspect original settings and entry requirements; fresh capture and a reviewed execution path are still required.\",\"cam_ready\":false,\"machine_action_authorized\":false}}",
        record::quote(&output.display().to_string()),
        selection.cells.len(),
        selection.chosen.len(),
        selection.projections.len()
    ))
}

fn manifest(object: &Id, setup: &Id, id: &Id, r: &request::Request, json: &str) -> String {
    format!(
        "{{\"schema\":\"dmc2.spatial-observation-plan-bundle.v1\",\"object\":{},\"setup\":{},\"analysis\":{},\"material_analysis\":{},\"source_capture\":{},\"result\":{},\"cam_ready\":false,\"machine_action_authorized\":false}}\n",
        record::quote(object.as_str()),
        record::quote(setup.as_str()),
        record::quote(id.as_str()),
        record::quote(r.material.as_str()),
        record::quote(r.capture.as_str()),
        json
    )
}
