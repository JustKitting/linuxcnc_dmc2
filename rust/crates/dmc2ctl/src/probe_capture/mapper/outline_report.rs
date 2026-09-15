//! Ordered tactile outline export. No assumed stock shape or radius offset.
use super::super::ledger::{self, Fields};
use super::{
    model::{xyz, LocalSearch, Phase, Settings},
    outline,
    report::quoted,
    search::Progress,
    state,
};
use std::path::Path;

pub fn export(
    path: &Path,
    records: &[Fields],
    settings: &Settings,
    require_result: bool,
) -> Result<(), String> {
    let (replay, mut issue) = match state::samples(records, settings, require_result) {
        Ok(samples) => {
            let traced = outline::run(settings, &samples);
            let issue=match &traced.result {
                Ok(())=>None,
                Err(Progress::Need(_))=>Some("The local outline trace is unfinished; partial contacts remain observations. Use Abort then Pendant Mode.".into()),
                Err(Progress::Invalid(e))=>Some(e.clone()),
            };
            (Some(traced), issue)
        }
        Err(e) => (None, Some(e)),
    };
    if require_result {
        if let Some(e) = &issue {
            return Err(e.clone());
        }
    }
    let csv = super::report::event_csv(records)?;
    let mut trial_points = Vec::new();
    let mut trial_sequences = Vec::<usize>::new();
    for record in &records[1..] {
        let phase = Phase::read(ledger::number(record, "phase")?)?;
        if record["kind"] == "touch"
            && record["stage"] == "1"
            && matches!(
                phase,
                Phase::OutlineEnter | Phase::OutlineAdvance | Phase::OutlineClose
            )
        {
            trial_points.push(xyz(record, "machine_", "_exact")?);
            trial_sequences.push(record["sequence"].parse().map_err(|_|"Invalid original fine-contact sequence; preserve the ledger and import an intact capture.")?);
        }
    }
    let ended = records.last().is_some_and(|r| r["kind"] == "result");
    let closed = issue.is_none() && ended;
    if issue.is_none() && !ended {
        issue=Some("The seam check is retained but the final program result is missing; the run remains partial.".into());
    }
    let adaptive = settings
        .outline
        .is_some_and(|p| matches!(p.local_search, LocalSearch::GrowingRefinement));
    let (points, sequences, selection) = match &replay {
        Some(traced) => (
            traced.points.as_slice(),
            traced.sequences.as_slice(),
            "retained-policy-replay",
        ),
        None if !adaptive => (
            trial_points.as_slice(),
            trial_sequences.as_slice(),
            "legacy-acquisition-order-diagnostic",
        ),
        None => (
            [].as_slice(),
            [].as_slice(),
            "unavailable-invalid-capture-cycle",
        ),
    };
    let plane = if let Some(z) = replay.as_ref().and_then(|r| r.plane) {
        Some(z)
    } else {
        records
            .iter()
            .rev()
            .find(|r| {
                r["kind"] == "touch"
                    && r["stage"] == "1"
                    && matches!(r["phase"].as_str(), "0" | "1")
            })
            .map(|r| {
                ledger::number(r, "machine_z_exact")
                    .map(|z| settings.trace_z(z - settings.offset[2]))
            })
            .transpose()?
    };
    let refinements = replay
        .as_ref()
        .map(|r| {
            r.refinements
                .iter()
                .map(outline::Refinement::json)
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    let report=format!("{{\n\"schema\":\"dmc2.tactile-outline.v2\",\n\"program_result_present\":{ended},\n\"closed_with_fine_seam_check\":{closed},\n\"issue\":{},\n\"frame\":\"LinuxCNC original machine trigger XYZ in mm\",\n\"work_to_machine_translation_mm\":{:?},\n\"trace_work_z_mm\":{},\n\"initial_local_radius_mm\":{},\n\"resolution_mm\":{},\n\"outline_policy_snapshot\":{},\n\"selection_source\":{},\n\"ordered_fine_contact_sequences\":{:?},\n\"ordered_fine_contacts_machine_mm\":{:?},\n\"fine_trial_sequences\":{:?},\n\"fine_trial_contacts_machine_mm\":{:?},\n\"refinements\":[{refinements}],\n\"interpretation\":\"Original fine triggers and measured midpoint decisions. Ordered selection may differ from acquisition order; all trials remain in the ledger and event CSV. Resolved means the sampled midpoint met the chosen chord tolerance, not that unsampled material or a solid volume is established.\",\n\"cam_ready\":false\n}}\n",issue.as_deref().map(quoted).unwrap_or_else(||"null".into()),settings.offset,plane.map(|z|z.to_string()).unwrap_or_else(||"null".into()),settings.grid,settings.resolution,quoted(&path.with_extension("outline.txt").display().to_string()),quoted(selection),sequences,points,trial_sequences,trial_points);
    let stem = if require_result {
        String::new()
    } else {
        format!("partial-{}.", records.len())
    };
    ledger::publish(&path.with_extension(format!("{stem}csv")), csv.as_bytes())?;
    ledger::publish(
        &path.with_extension(format!("{stem}json")),
        report.as_bytes(),
    )
}
