//! Original top observations, sampling brackets and explicitly unknown coverage.
use super::{
    free_surface::BoundarySide,
    model::{Phase, Settings},
    report::{event_csv, quoted},
    search::{Progress, Survey},
    state,
};
use dmc2ctl::probe_data::ledger::{self, number, Fields};
use std::path::Path;

fn coverage(survey: &Survey<'_>) -> String {
    let brackets = survey.brackets.iter().map(|b| format!(
        "{{\"hit_record\":{},\"miss_record\":{},\"hit_requested_work_xy_mm\":{:?},\"miss_requested_work_xy_mm\":{:?},\"requested_gap_mm\":{},\"within_requested_resolution\":{}}}",
        b.hit_sequence, b.miss_sequence, b.hit_xy, b.miss_xy, b.gap, b.gap <= survey.s.resolution
    )).collect::<Vec<_>>().join(",");
    let contacts = survey.boundary_contacts.iter().map(|b| {
        let direction = match (b.axis, b.side) {
            (0, BoundarySide::Low) => "physical RIGHT / LinuxCNC -X",
            (0, BoundarySide::High) => "physical LEFT / LinuxCNC +X",
            (_, BoundarySide::Low) => "LinuxCNC -Y",
            (_, BoundarySide::High) => "LinuxCNC +Y",
        };
        format!("{{\"direction\":{},\"fine_record\":{},\"requested_work_xy_mm\":{:?},\"stock_edge_established\":false}}", quoted(direction), b.sequence, b.requested_xy)
    }).collect::<Vec<_>>().join(",");
    let censored: Vec<_> = survey.censored_grid.iter().map(|&(x, y)| [x, y]).collect();
    format!("{{\"hit_requested_work_xy_mm\":{:?},\"miss_requested_work_xy_mm\":{:?},\"brackets\":[{brackets}],\"plate_boundary_contacts\":[{contacts}],\"unmeasured_censored_grid_indices\":{censored:?}}}", survey.hits, survey.misses)
}

pub(super) fn export(
    path: &Path,
    records: &[Fields],
    settings: &Settings,
    require_result: bool,
) -> Result<(), String> {
    let (geometry, mut issue, planned) = match state::samples(records, settings, require_result) {
        Ok(samples) => {
            let mut survey = Survey::new(settings, &samples);
            let result = survey.free_surface();
            let issue = match &result {
                Ok(()) => None,
                Err(Progress::Need(_)) => Some("Automatic top acquisition is unfinished; these are partial observations. Use Abort then Pendant Mode before a new Run.".into()),
                Err(Progress::Invalid(e)) => Some(e.clone()),
            };
            (coverage(&survey), issue, result.is_ok())
        }
        Err(e) => ("null".into(), Some(e), false),
    };
    let ended = records.last().is_some_and(|r| r["kind"] == "result");
    if issue.is_none() && !ended {
        issue = Some("The planned top samples are retained but the final program result is missing. Preserve the partial run; it is not a completed acquisition.".into());
    }
    if require_result {
        if let Some(e) = &issue {
            return Err(e.clone());
        }
    }
    let mut fine = 0;
    let mut checks = 0;
    let mut misses = 0;
    for r in &records[1..] {
        if r["kind"] == "touch" && number(r, "stage")? == 1.0 {
            fine += 1;
            if Phase::read(number(r, "phase")?)? == Phase::Verify {
                checks += 1;
            }
        }
        if r["kind"] == "miss" {
            misses += 1;
        }
    }
    let report = format!("{{\n\"schema\":\"dmc2.automatic-top-map.v1\",\n\"program_result_present\":{ended},\n\"planned_samples_retained\":{planned},\n\"issue\":{},\n\"trigger_frame\":\"Original LinuxCNC machine trigger XYZ in mm; see event CSV and source ledger\",\n\"work_to_machine_translation_mm\":{:?},\n\"starting_work_xyz_mm\":{:?},\n\"fixed_floor_work_z_mm\":{},\n\"grid_mm\":{},\n\"boundary_bracket_resolution_mm\":{},\n\"nominal_ball_diameter_mm\":{},\n\"plate_snapshot\":{},\n\"feed_snapshot\":{},\n\"retained_fine_trigger_records\":{fine},\n\"independent_cell_centre_fine_records\":{checks},\n\"retained_coarse_miss_records\":{misses},\n\"coverage\":{geometry},\n\"interpretation\":\"Connected sampled top contact domain at the original fixed search floor. Fresh cell-centre fine contacts are independent checks for stock-surface fitting. Requested XY brackets are sampling locations, not corrected physical edges. Exact triggers remain in the ledger and CSV. Plate-censored neighbours, disconnected areas and unsampled gaps remain unknown. Misses have no assigned height and do not certify empty volume. No rectangle, solid volume, radius correction, tool offset or absolute plate Z is inferred here.\",\n\"cam_ready\":false\n}}\n",
        issue.as_deref().map(quoted).unwrap_or_else(|| "null".into()), settings.offset, settings.origin, settings.floor,
        settings.grid, settings.resolution, 2.0 * settings.radius,
        quoted(&path.with_extension("plate.txt").display().to_string()), quoted(&path.with_extension("feeds.txt").display().to_string()));
    let stem = if require_result {
        String::new()
    } else {
        format!("partial-{}.", records.len())
    };
    ledger::publish(
        &path.with_extension(format!("{stem}csv")),
        event_csv(records)?.as_bytes(),
    )?;
    ledger::publish(
        &path.with_extension(format!("{stem}json")),
        report.as_bytes(),
    )
}
