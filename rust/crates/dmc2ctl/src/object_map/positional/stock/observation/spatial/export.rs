//! Reproduce selected evidence needs, then export an explicit operator-run file.
use super::{material, report, request, select, source};
use crate::{
    object_map::{
        model::Id,
        positional::retained::Bundle,
        record,
        store::{save, Store},
        Error,
    },
    probe_data::top_followup::Plan,
};
use std::path::Path;

pub fn run(
    store: &Store,
    object: &Id,
    setup: &Id,
    id: &Id,
    output: &Path,
) -> Result<String, Error> {
    if output.exists() {
        return Err(Error::Storage("The follow-up output directory already exists. Choose a new directory; prior programs and captures are preserved.".into()));
    }
    let original = Bundle::read(&crate::object_map::positional::folder(
        store, object, setup, id,
    )?)?;
    let raw = original.get("request.txt")?;
    if !raw.starts_with(format!("{}\n", request::SCHEMA).as_bytes()) {
        return Err(Error::Input("This export requires a new-top-sample analysis. Repeat/side observation plans need their own reviewed entry paths; no selected observations were silently discarded. Select a spatial top analysis.".into()));
    }
    let r = request::Request::read(raw)?;
    let (material_source, a) = material::load(store, object, setup, &r.material)?;
    original.require_source(&material_source, "material-source-")?;
    let captures = store.captures(object, setup)?;
    let source = source(&a, &captures, &r.capture)?;
    let selection = select::run(&a, &source, &r)?;
    let report = report::build(&a, &source, &selection, &r)?;
    original.require_equal("observation-plan.machine-mm.json", report.json.as_bytes())?;
    original.require_equal("residuals.csv", report.csv.as_bytes())?;
    original.require_equal(
        "manifest.json",
        super::manifest(object, setup, id, &r, &report.json).as_bytes(),
    )?;
    let c = captures.iter().find(|c|c.id==r.capture).ok_or_else(||Error::Data("The original capture is missing. Import its intact ledger and companions before exporting.".into()))?;
    let snapshot = |name| -> Result<String, Error> {
        let bytes = c.context.snapshots().find(|(k,_)|*k==name).and_then(|(_,b)|b).ok_or_else(||Error::Data(format!("The original {name} snapshot is missing. Preserve the source and import its intact companions before exporting.")))?;
        String::from_utf8(bytes.to_vec())
            .map_err(|e| Error::Data(format!("Original {name} snapshot is not UTF-8: {e}.")))
    };
    let plan = Plan {
        source: [
            object.as_str().into(),
            setup.as_str().into(),
            id.as_str().into(),
            r.capture.as_str().into(),
        ],
        start: c.capture.records[0].clone(),
        plate: snapshot("plate")?,
        feeds: snapshot("feeds")?,
        points: selection
            .chosen
            .iter()
            .map(|&i| selection.cells[i].column.request.approach)
            .collect(),
    };
    let encoded = plan.encode().map_err(Error::Data)?;
    let program = plan.program().map_err(Error::Data)?;
    Plan::from_program(&program).map_err(Error::Data)?;
    let s = plan.settings().map_err(Error::Data)?;
    let mut rows = String::from("row,spatial_cell,work_x_mm,work_y_mm,clear_work_z_mm,floor_work_z_mm,downward_feed_mm_min,fine_feed_mm_min,travel_feed_mm_min\n");
    for (row, &cell) in selection.chosen.iter().enumerate() {
        let xy = plan.points[row];
        rows.push_str(&format!(
            "{row},{cell},{},{},{},{},{},{},{}\n",
            xy[0], xy[1], s.origin[2], s.floor, s.downward_feed, s.feeds[1], s.feeds[2]
        ));
    }
    let instructions = format!("Top follow-up from object {}, setup {}, analysis {}.\n\nReview the original stock/probe reference and clear transfer plane for this placement. Required starting work XYZ mm: {:?}. Work-to-machine translation mm: {:?}. The program checks retained numerical starting fields before publishing a target; these numbers do not establish physical alignment. No entry-positioning move is supplied. Each selected column transfers at original clearance, dips to the original floor, double-touches on coarse contact, records original fine trigger coordinates and withdraws using mapper-run. A coarse miss has no assigned surface height.\n\nThe explicit execution order is execution-order.csv, matching the analysis priority order. No path optimization or additional points are inserted. X increasing: physical LEFT / LinuxCNC +X; decreasing: physical RIGHT / LinuxCNC -X.\n\nAfter review, use AXIS File Open on top-followup.ngc and the normal Run control. Exporting does not load or run it. The exact plan is embedded in that program and is copied with its plate/feed snapshots beside the fresh tmp/output/mapper ledger. A separate .followup.txt is retained here for inspection; editing it does not change the executable program.\n\nImport the fresh ledger with companions into this object/setup under a new capture ID, then Prepare 3D stock surfaces. Fresh top contacts supply fit rows; prior withheld check roles remain unchanged. Calculate a new surface/material assessment to select further evidence. Side, underside and unmeasured volume remain unresolved.\n\nAbort, Clear Fault and Pendant Mode remain the standard recovery controls. An interrupted cycle or mismatched plan/frame cannot supply follow-up material evidence. Preserve its ledger and diagnosis before another operator-run acquisition.\n",object.as_str(),setup.as_str(),id.as_str(),s.origin,s.offset);
    // Publish the program last: a partially written export is not runnable.
    original.copy_to(output, "observation-source-")?;
    save(
        &output.join("top-followup.followup.txt"),
        encoded.as_bytes(),
    )?;
    save(&output.join("execution-order.csv"), rows.as_bytes())?;
    save(&output.join("README.txt"), instructions.as_bytes())?;
    save(&output.join("top-followup.ngc"), program.as_bytes())?;
    Ok(format!("{{\"program\":{},\"execution_order\":{},\"requested_rows\":{},\"state\":\"exported-for-entry-and-program-review\",\"message\":\"Review the original frame/start and every clearance transfer, then use standard File Open/Run. Import the fresh ledger with its companions for a new stock-surface/material calculation. No machine command was issued.\",\"machine_action_authorized\":false,\"cam_ready\":false}}",record::quote(&output.join("top-followup.ngc").display().to_string()),record::quote(&output.join("execution-order.csv").display().to_string()),plan.points.len()))
}
