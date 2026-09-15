//! New material-directed top columns through the normal Object Mapper path.
mod grid;
mod report;
pub(in crate::object_map) mod request;
mod select;
#[cfg(test)]
mod tests;
use super::material;
use crate::{
    object_map::{
        Error,
        model::{CaptureState, Id},
        record,
        store::{CaptureSnapshot, Store},
    },
    probe_data::{
        mapper_settings::{Phase, Sample, Settings, close},
        mapper_trace::{observation::TopColumn, state},
        schema::Workflow,
    },
};
use std::path::Path;

struct Source {
    settings: Settings,
    samples: Vec<Sample>,
}
fn source(
    a: &material::Assessment,
    captures: &[CaptureSnapshot],
    id: &Id,
) -> Result<Source, Error> {
    if !a.surface.contacts.iter().any(|s| s.capture == *id) {
        return Err(Error::Input("The selected top capture contributes no retained fine contacts to this material assessment. Select a contributing capture or calculate a new source surface/material analysis.".into()));
    }
    let c = captures.iter().find(|c| c.id == *id).ok_or_else(|| Error::Data("The selected original top capture is absent from this setup. Import the intact capture and its original companions before preparing new samples.".into()))?;
    a.surface
        .source
        .require_equal(&format!("capture-{}.txt", id.as_str()), &c.raw)?;
    a.surface.source.check_context(c)?;
    if c.capture.state == CaptureState::Quarantined || c.capture.workflow != Workflow::Mapper {
        return Err(Error::Data("The selected capture is not an eligible mapper run. Retain its diagnosis and select a non-quarantined top capture with complete original cycles.".into()));
    }
    let settings = c.context.settings(&c.capture).map_err(Error::Data)?;
    TopColumn::new(&settings, [settings.origin[0], settings.origin[1]]).map_err(Error::Data)?;
    if !close(settings.radius, a.surface.request.probe.radius) {
        return Err(Error::Data("The retained acquisition ball radius disagrees with the surface analysis probe model. Resolve those original references before planning; no radius or travel envelope was substituted.".into()));
    }
    let samples = state::samples(&c.capture.records, &settings, false).map_err(|e| Error::Data(format!("Cannot use capture {} for new top samples: {e} Preserve its diagnosis and select an intact capture with complete cycles.",id.as_str())))?;
    if samples.is_empty() {
        return Err(Error::Data("The selected capture contains no complete top-search cycles. Preserve its original ledger and select a capture with retained cycles before planning.".into()));
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
                id.as_str(),
                sample.sequence
            )));
        }
    }
    Ok(Source { settings, samples })
}
pub fn prepare(
    store: &Store,
    object: &Id,
    setup: &Id,
    material: &Id,
    capture: &Id,
) -> Result<String, Error> {
    let (_, a) = material::load(store, object, setup, material)?;
    source(&a, &store.captures(object, setup)?, capture)?;
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
    String::from_utf8(record::encode(request::SCHEMA, &fields, &[])?)
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
    let source = source(&a, &store.captures(object, setup)?, &r.capture)?;
    let selection = select::run(&a, &source, &r)?;
    let report = report::build(&a, &source, &selection, &r)?;
    let manifest = format!(
        "{{\"schema\":\"dmc2.spatial-observation-plan-bundle.v1\",\"object\":{},\"setup\":{},\"analysis\":{},\"material_analysis\":{},\"source_capture\":{},\"result\":{},\"cam_ready\":false,\"machine_action_authorized\":false}}\n",
        record::quote(object.as_str()),
        record::quote(setup.as_str()),
        record::quote(id.as_str()),
        record::quote(r.material.as_str()),
        record::quote(r.capture.as_str()),
        report.json
    );
    super::retain(output, raw, &bundle, &report, &manifest)?;
    Ok(format!(
        "{{\"analysis_directory\":{},\"state\":\"unreviewed-spatial-observation-proposals\",\"spatial_cells\":{},\"selected_observations\":{},\"unsupported_material_regions\":{},\"message\":\"New top-column proposals and all unresolved material regions are retained. Inspect original settings and entry requirements; fresh capture and a reviewed execution path are still required.\",\"cam_ready\":false,\"machine_action_authorized\":false}}",
        record::quote(&output.display().to_string()),
        selection.cells.len(),
        selection.chosen.len(),
        selection.projections.len()
    ))
}
