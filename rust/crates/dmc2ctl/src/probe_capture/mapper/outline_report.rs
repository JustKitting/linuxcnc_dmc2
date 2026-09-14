//! Ordered tactile outline export. No assumed rectangular stock or radius offset.
use super::super::ledger::{self, Fields};
use super::{
    model::{xyz, Phase, Settings},
    outline,
    report::quoted,
    state,
};
use std::path::Path;

pub fn export(
    path: &Path,
    records: &[Fields],
    settings: &Settings,
    require_result: bool,
) -> Result<(), String> {
    let result = state::samples(records, settings, require_result).and_then(|samples| {
        match outline::run(settings, &samples) {
            Ok(value) => Ok(value),
            Err(super::search::Progress::Invalid(e)) => Err(e),
            Err(super::search::Progress::Need(_)) => {
                Err("The local outline trace is unfinished.".into())
            }
        }
    });
    if require_result {
        result.as_ref().map_err(Clone::clone)?;
    }
    let csv = super::report::event_csv(records)?;
    let mut partial_points = Vec::new();
    for record in &records[1..] {
        let phase = Phase::read(ledger::number(record, "phase")?)?;
        if record["kind"] == "touch"
            && record["stage"] == "1"
            && matches!(
                phase,
                Phase::OutlineEnter | Phase::OutlineAdvance | Phase::OutlineClose
            )
        {
            partial_points.push(xyz(record, "machine_", "_exact")?);
        }
    }
    let ended = records.last().is_some_and(|r| r["kind"] == "result");
    let closed = result.is_ok() && ended;
    let (points, plane) = match &result {
        Ok(outline) => (&outline.points, outline.plane.to_string()),
        Err(_) => {
            let top = records.iter().rev().find(|r| {
                r["kind"] == "touch"
                    && r["stage"] == "1"
                    && matches!(r["phase"].as_str(), "0" | "1")
            });
            let plane = top
                .map(|r| {
                    ledger::number(r, "machine_z_exact")
                        .map(|z| (z - settings.offset[2]).to_string())
                })
                .transpose()?
                .unwrap_or_else(|| "null".into());
            (&partial_points, plane)
        }
    };
    let issue = result.as_ref().err().map(|s| quoted(s)).unwrap_or_else(|| {
        if ended { "null".into() } else { quoted("The seam check is retained but the final program result is missing; the run remains partial.") }
    });
    let report = format!("{{\n\"schema\":\"dmc2.tactile-outline.v1\",\n\"program_result_present\":{ended},\n\"closed_with_fine_seam_check\":{closed},\n\"issue\":{issue},\n\"frame\":\"LinuxCNC original machine trigger XYZ in mm\",\n\"work_to_machine_translation_mm\":{:?},\n\"trace_work_z_mm\":{plane},\n\"local_radius_mm\":{},\n\"resolution_mm\":{},\n\"outline_policy_snapshot\":{},\n\"ordered_fine_contacts_machine_mm\":{:?},\n\"interpretation\":\"Measured probe-trigger contour at fixed Z. No rectangle fit, unmeasured opposite edge, ball-radius offset, tool-offset change or presumed solid geometry is substituted for these ordered contacts.\",\n\"cam_ready\":false\n}}\n",settings.offset,settings.grid,settings.resolution,quoted(&path.with_extension("outline.txt").display().to_string()),points);
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
