use super::super::{
    ledger::{self, number},
    schema::Workflow,
};
use super::{
    geometry::{dot, plane, Plane},
    model::{trigger, Phase, State},
};
use std::{fs, path::Path};

fn plane_json(p: Option<Plane>) -> String {
    match p {
        None => "null".into(),
        Some(p) => format!(
            "{{\"origin\":{:?},\"slopes_mm_per_mm\":{:?},\"rms_mm\":{},\"count\":{}}}",
            p.origin, p.slopes, p.rms, p.count
        ),
    }
}

pub(crate) fn export(path: &Path, require_result: bool) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("Reading block ledger: {e}"))?;
    let records = ledger::records(&text, Workflow::Block)?;
    let has_result = records.last().map(|r| r["kind"].as_str()) == Some("result");
    if require_result && !has_result {
        return Err("No terminal block result was retained.".into());
    }
    let before_result = if has_result {
        &records[..records.len() - 1]
    } else {
        &records[..]
    };
    let state = State::read(before_result)?;
    if has_result && state.next()?.phase != Phase::Finished {
        return Err("The block result precedes a complete grid boundary and three full side circuits; derived results are quarantined.".into());
    }
    let mut csv = String::from("sequence,kind,sample,phase,column,row,edge,layer,stage,source,machine_x_mm,machine_y_mm,machine_z_mm,from_work_x_mm,from_work_y_mm,from_work_z_mm,target_work_x_mm,target_work_y_mm,target_work_z_mm,direction_x,direction_y,direction_z,commanded_feed_mm_min\n");
    for r in records.iter().skip(1) {
        let is_trigger = matches!(r["kind"].as_str(), "touch" | "obstruction");
        let xyz = if is_trigger {
            trigger(r)?
        } else {
            super::model::point(r, "machine_")?
        };
        let from = super::model::point(r, "from_")?;
        let to = super::model::point(r, "target_")?;
        let vector = [0, 1, 2].map(|i| to[i] - from[i]);
        let norm = vector.iter().map(|v| v * v).sum::<f64>().sqrt();
        let direction = vector.map(|v| if norm > 0.0 { v / norm } else { 0.0 });
        let source = if is_trigger {
            "original_G38_trigger_f64"
        } else {
            "reported_endpoint_not_a_trigger"
        };
        let ids = [
            "sequence", "kind", "sample", "phase", "column", "row", "edge", "layer", "stage",
        ]
        .map(|k| r[k].as_str())
        .join(",");
        let values = xyz
            .into_iter()
            .chain(from)
            .chain(to)
            .chain(direction)
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");
        csv.push_str(&format!("{ids},{source},{values},{}\n", r["feed"]));
    }
    // Border ball contacts are retained in CSV but not mislabelled as top-flat
    // samples. Plane fitting uses contacts surrounded by top hits on the grid.
    let mut interior = Vec::new();
    for r in &state.top {
        let cell = (number(r, "column")? as i32, number(r, "row")? as i32);
        if (-1..=1)
            .all(|dx| (-1..=1).all(|dy| state.grid.get(&(cell.0 + dx, cell.1 + dy)) == Some(&true)))
        {
            interior.push(trigger(r)?);
        }
    }
    let mut sides = Vec::new();
    let (footprint, footprint_issue) = match state.footprint() {
        Ok(rectangle) => (Some(rectangle), "null".to_owned()),
        Err(error) => (
            None,
            format!(
                "\"{}\"",
                error
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
            ),
        ),
    };
    let footprint_json = footprint.map(|r| format!("{{\"work_frame_basis_u\":{:?},\"work_frame_basis_v\":{:?},\"support_min_mm\":{:?},\"support_max_mm\":{:?},\"contact_envelope_only\":true}}", r.u, r.v, r.min, r.max)).unwrap_or_else(|| "null".into());
    if let Some(rectangle) = footprint {
        for edge in 0..4 {
            let normal =
                [rectangle.u, rectangle.v][edge / 2].map(|v| if edge % 2 == 0 { -v } else { v });
            let tangent = [-normal[1], normal[0]];
            let mut points = Vec::new();
            for r in &state.sides {
                if number(r, "edge")? != edge as f64 {
                    continue;
                }
                let [x, y, z] = trigger(r)?;
                points.push([dot([x, y], tangent), z, dot([x, y], normal)]);
            }
            sides.push(format!("{{\"edge\":{edge},\"normal_xy\":{normal:?},\"tangent_xy\":{tangent:?},\"fit\":{}}}", plane_json(plane(&points))));
        }
    }
    let metadata = format!("{{\n\"footprint\":{footprint_json},\n\"footprint_issue\":{footprint_issue},\n\"schema\":\"dmc2.gauge-block.v1\",\n\"program_result_present\":{has_result},\n\"obstruction_or_side_miss\":{},\n\"frame\":\"LinuxCNC machine trigger coordinates in mm\",\n\"ball_diameter_nominal_mm\":{},\n\"grid_cells\":{},\n\"fine_top_contacts\":{},\n\"fine_side_contacts\":{},\n\"absolute_tool_or_probe_offset_applied\":false,\n\"top_plane_formula\":\"z=origin_z+a*(x-origin_x)+b*(y-origin_y); interior grid hits only\",\n\"top_plane\":{},\n\"side_plane_formula\":\"n dot xy=origin_w+a*(t dot xy-origin_u)+b*(z-origin_v); b is relative side lean, a is relative yaw\",\n\"side_planes\":[{}],\n\"interpretation\":\"Relative block/probe/axis geometry. Block placement, probe deflection and machine geometry are not independently separated; no plate-flatness or spindle-only correction is applied. Misses have no assigned surface height.\"\n}}\n",
        state.failed, number(state.start, "ball_diameter")?, state.grid.len(), state.top.len(), state.sides.len(), plane_json(plane(&interior)), sides.join(","));
    // A partial export has its own sequence-qualified name; it cannot prevent
    // the eventual complete export or overwrite another retained snapshot.
    let stem = if has_result {
        path.to_owned()
    } else {
        path.with_file_name(format!(
            "{}-partial-{}",
            path.file_stem()
                .ok_or("Block ledger has no name")?
                .to_string_lossy(),
            records.len()
        ))
    };
    ledger::publish(&stem.with_extension("csv"), csv.as_bytes())?;
    ledger::publish(&stem.with_extension("json"), metadata.as_bytes())?;
    println!(
        "Retained gauge-block coordinates and relative plane fits: {}",
        stem.with_extension("json").display()
    );
    Ok(())
}
