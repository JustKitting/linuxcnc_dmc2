//! One immutable top-program publisher for new columns and original repeats.
use super::{repeat, spatial};
use crate::{
    object_map::{
        model::Id,
        positional::retained::Bundle,
        record,
        store::{save, Store},
        Error,
    },
    probe_data::top_followup::{Plan, Rows},
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
    let (plan, order) = if spatial::request::recognizes(original.get("request.txt")?) {
        spatial::export::plan(store, object, setup, id, &original)?
    } else {
        repeat::plan(store, object, setup, id, &original)?
    };
    let encoded = plan.encode().map_err(Error::Data)?;
    let program = plan.program().map_err(Error::Data)?;
    Plan::from_program(&program).map_err(Error::Data)?;
    let s = plan.settings().map_err(Error::Data)?;
    let role_header = if plan.role.is_some() {
        ",contact_role"
    } else {
        ""
    };
    let role_value = plan
        .role
        .map(|r| format!(",{}", r.name()))
        .unwrap_or_default();
    let index_column = if plan.rows.repeated() {
        "proposal"
    } else {
        "spatial_cell"
    };
    let repeat_header = if plan.rows.repeated() {
        ",source_capture,source_sequence,original_phase"
    } else {
        ""
    };
    let requests = plan.requests().map_err(Error::Data)?;
    if requests.len() != order.len() {
        return Err(Error::Data("The exported row order differs from its retained plan. Preserve the analysis and re-export; no program was written.".into()));
    }
    let mut rows = format!("row,{index_column},work_x_mm,work_y_mm,clear_work_z_mm,floor_work_z_mm,downward_feed_mm_min,fine_feed_mm_min,travel_feed_mm_min{role_header}{repeat_header}\n");
    for (row, &cell) in order.iter().enumerate() {
        let xy = requests[row].approach;
        let repeat_value = match &plan.rows {
            Rows::Repeats(repeats) => {
                let r = &repeats[row];
                format!(",{},{},{}", r.capture, r.sequence, r.request.phase as u8)
            }
            Rows::Columns(_) | Rows::Directed(_) => String::new(),
        };
        rows.push_str(&format!(
            "{row},{cell},{},{},{},{},{},{},{}{role_value}{repeat_value}\n",
            xy[0], xy[1], s.origin[2], s.floor, s.downward_feed, s.feeds[1], s.feeds[2]
        ));
    }
    let contact_instructions = plan.role.map(|r| r.description()).unwrap_or(
        "Fresh top contacts supply fit rows; prior withheld check roles remain unchanged.",
    );
    let instructions = format!(
        "Top follow-up from object {}, setup {}, analysis {}.\n\nReview the original stock/probe reference and clear transfer plane for this placement. Required starting work XYZ mm: {:?}. Work-to-machine translation mm: {:?}. The program checks retained numerical starting fields before publishing a target; these numbers do not establish physical alignment. No entry-positioning move is supplied. Each selected column transfers at original clearance, dips to the original floor, double-touches on coarse contact, records original fine trigger coordinates and withdraws using mapper-run. A coarse miss has no assigned surface height.\n\nThe explicit execution order is execution-order.csv, matching the analysis priority order. No path optimization or additional points are inserted. X increasing: physical LEFT / LinuxCNC +X; decreasing: physical RIGHT / LinuxCNC -X.\n\nAfter review, use AXIS File Open on top-followup.ngc and the normal Run control. Exporting does not load or run it. The exact plan is embedded in that program and is copied with its plate/feed snapshots beside the fresh tmp/output/mapper ledger. A separate .followup.txt is retained here for inspection; editing it does not change the executable program.\n\nImport the fresh ledger with companions into this object/setup under a new capture ID, then Prepare 3D stock surfaces. {contact_instructions} Calculate a new surface/material assessment to select further evidence. Side, underside and unmeasured volume remain unresolved.\n\nAbort, Clear Fault and Pendant Mode remain the standard recovery controls. An interrupted cycle or mismatched plan/frame cannot supply follow-up material evidence. Preserve its ledger and diagnosis before another operator-run acquisition.\n",
        object.as_str(),
        setup.as_str(),
        id.as_str(),
        s.origin,
        s.offset
    );
    // Publish the program last: a partially written export is not runnable.
    original.copy_to(output, "observation-source-")?;
    save(
        &output.join("top-followup.followup.txt"),
        encoded.as_bytes(),
    )?;
    save(&output.join("execution-order.csv"), rows.as_bytes())?;
    save(&output.join("README.txt"), instructions.as_bytes())?;
    save(&output.join("top-followup.ngc"), program.as_bytes())?;
    Ok(format!(
        "{{\"program\":{},\"execution_order\":{},\"requested_rows\":{},\"state\":\"exported-for-entry-and-program-review\",\"message\":\"Review the original frame/start and every clearance transfer, then use standard File Open/Run. Import the fresh ledger with its companions for a new stock-surface/material calculation. No machine command was issued.\",\"machine_action_authorized\":false,\"cam_ready\":false}}",
        record::quote(&output.join("top-followup.ngc").display().to_string()),
        record::quote(&output.join("execution-order.csv").display().to_string()),
        plan.rows.len()
    ))
}
