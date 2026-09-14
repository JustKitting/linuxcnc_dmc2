//! Export observed trigger envelopes and explicit partial/confirmed status.
use super::super::{
    ledger::{self, number},
    schema::Workflow,
};
use super::{
    model::{xyz, Mode},
    read,
    search::{Progress, Survey},
    state,
};
use std::path::Path;

pub(super) fn quoted(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(crate) fn export(path: &Path, require_result: bool) -> Result<(), String> {
    let (records, settings) = read(path)?;
    if settings.mode == Mode::Outline {
        return super::outline_report::export(path, &records, &settings, require_result);
    }
    let result = state::samples(&records, &settings, require_result).and_then(|samples| {
        let mut survey = Survey::new(&settings, &samples);
        match survey.run() {
            Ok(rect) => Ok(rect),
            Err(Progress::Invalid(e)) => Err(e),
            Err(Progress::Need(_)) => Err("The planned measurements are unfinished.".into()),
        }
    });
    if require_result {
        result.as_ref().map_err(Clone::clone)?;
    }
    let csv = event_csv(&records)?;
    let ended = records.last().is_some_and(|r| r["kind"] == "result");
    let issue = result
        .as_ref()
        .err()
        .map(|e| quoted(e))
        .unwrap_or_else(|| "null".into());
    let estimate = match result.as_ref() {
        Ok(rect) => super::dimensions::Estimate::read(&records, &settings, *rect)?
            .map(|e| e.json())
            .unwrap_or_else(|| "null".into()),
        Err(_) => "null".into(),
    };
    let geometry = result.map(|r| format!("{{\"basis_u\":{:?},\"basis_v\":{:?},\"support_min_work_mm\":{:?},\"support_max_work_mm\":{:?},\"contact_envelope_dimensions_mm\":[{},{}]}}",
        r.u,r.v,r.min,r.max,r.max[0]-r.min[0],r.max[1]-r.min[1])).unwrap_or_else(|_|"null".into());
    let hits = records
        .iter()
        .filter(|r| r["kind"] == "touch" && r.get("stage").is_some_and(|s| s == "1"))
        .count();
    let misses = records.iter().filter(|r| r["kind"] == "miss").count();
    let metadata = format!("{{\n\"schema\":\"dmc2.automatic-stock-map.v1\",\n\"mode\":\"{}\",\n\"program_result_present\":{ended},\n\"issue\":{issue},\n\"frame\":\"LinuxCNC machine trigger XYZ in mm\",\n\"work_to_machine_translation_mm\":{:?},\n\"starting_work_xyz_mm\":{:?},\n\"fixed_floor_work_z_mm\":{},\n\"maximum_downward_budget_mm\":{},\n\"plate_snapshot\":{},\n\"feed_snapshot\":{},\n\"fine_contacts\":{hits},\n\"misses\":{misses},\n\"grid_mm\":{},\n\"boundary_bracket_resolution_mm\":{},\n\"nominal_ball_diameter_mm\":{},\n\"footprint\":{geometry},\n\"nominal_stock_dimensions\":{estimate},\n\"interpretation\":\"Rough connected contact envelope at the selected depth. Misses have no assigned height. Interior grid cells may be unresolved below the selected spacing. Top edge coordinates include the spherical tip contact envelope; no XY radius, pretravel, tool-length or absolute plate-Z correction is applied. A cuboid fit and independent inside/outside face checks do not establish arbitrary unseen geometry. Rim CSV contains original side triggers for later face fitting.\",\n\"cam_ready\":false\n}}\n",
        if settings.mode==Mode::Rim {"rim"} else {"surface"}, settings.offset,settings.origin,settings.floor,number(&records[0],"drop")?,
        quoted(&path.with_extension("plate.txt").display().to_string()),quoted(&path.with_extension("feeds.txt").display().to_string()),
        settings.grid,settings.resolution,2.0*settings.radius);
    let stem = if require_result {
        "".into()
    } else {
        format!("partial-{}.", records.len())
    };
    ledger::publish(&path.with_extension(format!("{stem}csv")), csv.as_bytes())?;
    ledger::publish(
        &path.with_extension(format!("{stem}json")),
        metadata.as_bytes(),
    )
}

pub(super) fn event_csv(records: &[ledger::Fields]) -> Result<String, String> {
    let mut csv = String::from("sequence,sample,phase,edge,stage,kind,position_source,machine_x_mm,machine_y_mm,machine_z_mm,from_work_x_mm,from_work_y_mm,from_work_z_mm,target_work_x_mm,target_work_y_mm,target_work_z_mm,feed_mm_min\n");
    for r in records.iter().skip(1) {
        let trigger = Workflow::Mapper.requires_exact_trigger(&r["kind"]);
        let p = xyz(r, "machine_", if trigger { "_exact" } else { "" })?;
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            r["sequence"],
            r["sample"],
            r["phase"],
            r["edge"],
            r["stage"],
            r["kind"],
            if trigger {
                "original_G38_trigger"
            } else {
                "reported_endpoint_not_a_trigger"
            },
            p[0],
            p[1],
            p[2],
            r["from_x"],
            r["from_y"],
            r["from_z"],
            r["target_x"],
            r["target_y"],
            r["target_z"],
            r["feed"]
        ));
    }
    Ok(csv)
}
