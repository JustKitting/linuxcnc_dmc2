//! Replay the saved selection before binding every selected original top cycle.
use super::{acquisition, material, report, request, select};
use crate::{
    object_map::{model::Id, positional::retained::Bundle, store::Store, Error},
    probe_data::{
        mapper_settings::close,
        mapper_trace::observation::TopColumn,
        top_followup::{Plan, Repeat, Role, Rows},
    },
};

pub(super) fn plan(
    store: &Store,
    object: &Id,
    setup: &Id,
    id: &Id,
    original: &Bundle,
) -> Result<(Plan, Vec<usize>), Error> {
    let r = request::Request::read(original.get("request.txt")?)?;
    let (material_source, a) = material::load(store, object, setup, &r.material)?;
    original.require_source(&material_source, "material-source-")?;
    let captures = store.captures(object, setup)?;
    let selection = select::run(&a, &captures, &r)?;
    let report = report::build(&a, &selection)?;
    original.require_equal("observation-plan.machine-mm.json", report.json.as_bytes())?;
    original.require_equal("residuals.csv", report.csv.as_bytes())?;
    original.require_equal(
        "manifest.json",
        super::manifest(object, setup, id, &r, &report.json).as_bytes(),
    )?;
    let mut primary = None;
    let mut rows = Vec::new();
    for &proposal in &selection.chosen {
        let candidate = &selection.candidates[proposal];
        let source = &a.surface.contacts[candidate.sources[0]];
        let capture = captures.iter().find(|c| c.id == source.capture).ok_or_else(|| Error::Data(format!("Original capture {} is missing. Import its intact ledger and companions before exporting this repeat.", source.capture.as_str())))?;
        a.surface.source.require_equal(
            &format!("capture-{}.txt", capture.id.as_str()),
            &capture.raw,
        )?;
        a.surface.source.check_context(capture)?;
        let retained = candidate
            .proposal
            .as_ref()
            .map_err(|e| Error::Data(e.clone()))?;
        let run = selection.runs.get(source.capture.as_str()).ok_or_else(|| Error::Data("The selected repeat lost its source acquisition settings. Preserve the analysis and recalculate its original request before exporting.".into()))?.as_ref().map_err(|e| Error::Data(e.clone()))?;
        TopColumn::from_request(&run.settings, retained.request).map_err(|e| Error::Input(format!("Selected repeat proposal {proposal}, {}:{} cannot use the top executor: {e} No selected proposals were dropped and no program was exported.",source.capture.as_str(),source.sequence)))?;
        if !close(run.settings.radius, a.surface.request.probe.radius) {
            return Err(Error::Data("The repeat acquisition radius differs from its surface probe model. Resolve the original references before exporting; no measurement or envelope was substituted.".into()));
        }
        let first = *primary.get_or_insert(capture);
        acquisition::compatible(first, capture)?;
        rows.push(Repeat {
            proposal,
            capture: source.capture.as_str().into(),
            sequence: source.sequence,
            request: retained.request,
        });
    }
    let first = primary.ok_or_else(|| Error::Input("The saved observation analysis selected no repeat approaches. Inspect its unresolved requirements and prepare an analysis with eligible original cycles; no empty program was exported.".into()))?;
    let plan = Plan {
        source: [
            object.as_str(),
            setup.as_str(),
            id.as_str(),
            first.id.as_str(),
        ]
        .map(String::from),
        start: first.capture.records[0].clone(),
        plate: acquisition::snapshot(first, "plate")?,
        feeds: acquisition::snapshot(first, "feeds")?,
        rows: Rows::Repeats(rows),
        role: Some(Role::Check),
    };
    plan.settings().map_err(Error::Data)?;
    Ok((plan, selection.chosen))
}
