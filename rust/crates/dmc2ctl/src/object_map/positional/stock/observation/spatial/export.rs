//! Reproduce spatial selection; the common publisher owns program output.
use super::super::acquisition;
use super::{material, report, request, select, source};
use crate::{
    object_map::{model::Id, positional::retained::Bundle, store::Store, Error},
    probe_data::top_followup::{Plan, Rows},
};

pub(in crate::object_map::positional::stock::observation) fn plan(
    store: &Store,
    object: &Id,
    setup: &Id,
    id: &Id,
    original: &Bundle,
) -> Result<(Plan, Vec<usize>), Error> {
    let raw = original.get("request.txt")?;
    let r = request::Request::read(raw)?;
    let (material_source, a) = material::load(store, object, setup, &r.material)?;
    original.require_source(&material_source, "material-source-")?;
    let captures = store.captures(object, setup)?;
    let source = source::load(&a, &captures, &r)?;
    if matches!(&r.history, request::History::Explicit(_)) {
        source.check_retained(original)?;
    }
    let selection = select::run(&a, &source, &r)?;
    let report = report::build(&a, &source, &selection, &r)?;
    original.require_equal("observation-plan.machine-mm.json", report.json.as_bytes())?;
    original.require_equal("residuals.csv", report.csv.as_bytes())?;
    original.require_equal(
        "manifest.json",
        super::manifest(object, setup, id, &r, &report.json).as_bytes(),
    )?;
    let c = captures.iter().find(|c|c.id==r.capture).ok_or_else(||Error::Data("The original capture is missing. Import its intact ledger and companions before exporting.".into()))?;
    let plan = Plan {
        role: r.role,
        source: [
            object.as_str(),
            setup.as_str(),
            id.as_str(),
            r.capture.as_str(),
        ]
        .map(String::from),
        start: c.capture.records[0].clone(),
        plate: acquisition::snapshot(c, "plate")?,
        feeds: acquisition::snapshot(c, "feeds")?,
        rows: Rows::Columns(
            selection
                .chosen
                .iter()
                .map(|&i| selection.cells[i].column.request.approach)
                .collect(),
        ),
    };
    Ok((plan, selection.chosen))
}
