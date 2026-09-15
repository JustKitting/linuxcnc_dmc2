use super::super::{
    material::{self, query::State},
    report::Report,
};
use super::{
    request::{History, Request, Role},
    select::Selection,
    Source,
};
use crate::object_map::{
    positional::geometry::{add, finite},
    record::quote,
    Error,
};

pub fn build(
    a: &material::Assessment,
    source: &Source,
    selected: &Selection,
    r: &Request,
) -> Result<Report, Error> {
    let s = &source.settings;
    let mut cells = Vec::new();
    for (i, c) in selected.cells.iter().enumerate() {
        let p = c.column;
        let start_machine = add(p.start, s.offset);
        let target_machine = add(p.request.target, s.offset);
        if !finite(start_machine) || !finite(target_machine) {
            return Err(Error::Data("The proposed column cannot be represented in machine coordinates. Inspect original frame units and offsets before retrying.".into()));
        }
        let priority = selected
            .chosen
            .iter()
            .position(|&v| v == i)
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".into());
        let regions = c
            .regions
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let fields = p
            .request
            .values(s, 0, 0)
            .into_iter()
            .filter(|(k, _)| !matches!(*k, "sequence" | "sample"))
            .map(|(k, v)| format!("{}:{v}", quote(k)))
            .collect::<Vec<_>>()
            .join(",");
        // Historical V2 bytes used a work-frame label for the retained machine
        // trigger. New V3 names the frame correctly; source ledgers stay exact.
        let trigger_key = if r.role.is_some() {
            "original_trigger_machine_mm"
        } else {
            "original_trigger_work_mm"
        };
        let originals = match &r.history {
            History::LegacySingleSource => format!("\"already_searched_source_sequences\":{:?}",c.originals.iter().map(|o| o.sample.sequence).collect::<Vec<_>>()),
            History::Explicit(_) => format!("\"already_searched_records\":[{}]",c.originals.iter().map(|o| {
                let sample = &o.sample;
                let trigger = sample.trigger.map(|xyz| format!("{xyz:?}")).unwrap_or_else(|| "null".into());
                format!("{{\"capture\":{},\"sequence\":{},\"kind\":{},\"original_request_work_xy_mm\":{:?},\"{trigger_key}\":{trigger}}}",quote(o.capture.as_str()),sample.sequence,quote(if sample.trigger.is_some(){"fine-contact"}else{"coarse-miss"}),sample.request.approach)
            }).collect::<Vec<_>>().join(",")),
        };
        let role = r
            .role
            .map(|v| v.name())
            .unwrap_or("fit-or-observe-requires-new-capture");
        let check_support = if r.role.is_some() {
            format!(",\"check_predictions\":[{}],\"check_prediction_interpretation\":\"For check selection only: the upward-facing ball-centre plane intersects this original vertical column inside retained support and the original Z range. Work trigger = predicted centre - mounting vector - work translation, minus declared pretravel in Z for a downward approach. Predictions are not contacts, descent endpoints or proof of agreement. Unsupported candidates remain unselected.\"",c.checks.iter().map(|p| p.json(a)).collect::<Vec<_>>().join(","))
        } else {
            String::new()
        };
        cells.push(format!("{{\"cell\":{i},\"grid_index\":{:?},\"material_regions\":[{regions}],\"selected_priority\":{priority},{originals},\"nearest_original_request_xy_distance_mm\":{},\"state\":{},\"future_contact_role\":\"{role}\"{check_support},\"acquisition\":{{\"entry\":{},\"entry_requirement\":{},\"entry_work_mm\":{:?},\"entry_machine_mm\":{:?},\"target_machine_mm\":{:?},\"request_contract_fields\":{{{fields}}},\"new_trigger\":null,\"entry_path_supplied\":false,\"machine_action_authorized\":false}}}}",c.key,c.nearest_request_mm,quote(c.eligibility(r.role).name()),quote(p.entry.name()),quote(p.entry.recovery()),p.start,start_machine,target_machine));
    }
    let mut regions = Vec::new();
    let mut csv = String::from(
        "region,source_triangle,x_machine_mm,y_machine_mm,z_machine_mm,cover_radius_mm,material_state,spatial_cells,selected_top_columns,envelope_censored\n",
    );
    for (i, (cover, region)) in a.covers.iter().zip(&a.regions).enumerate() {
        let center = a.candidate.pose.point(cover.center);
        let p = selected.projections.get(&i);
        let ids = p.map(|p| p.cells.as_slice()).unwrap_or(&[]);
        let chosen = ids
            .iter()
            .filter(|id| selected.chosen.contains(id))
            .copied()
            .collect::<Vec<_>>();
        let (state, recovery) = region.state.description();
        let censored = p.map(|p| p.envelope_censored);
        csv.push_str(&format!(
            "{i},{},{},{},{},{},{},{},{},{}\n",
            cover.triangle,
            center[0],
            center[1],
            center[2],
            cover.radius,
            state,
            ids.len(),
            chosen.len(),
            censored.map(|v| v.to_string()).unwrap_or_default()
        ));
        if region.state != State::LocallyInward {
            let projection = p.map(|p|format!("{{\"center_machine_mm\":{:?},\"center_work_xy_mm\":{:?},\"envelope_censored\":{},\"spatial_cells\":{:?},\"selected_top_columns\":{:?},\"interpretation\":\"XY area of interest only; a new top touch does not by itself resolve 3D material support, side access or underside coverage.\"}}",p.center,p.work_xy,p.envelope_censored,p.cells,chosen)).unwrap_or_else(|| "null".into());
            regions.push(format!("{{\"region\":{i},\"source_triangle\":{},\"center_machine_mm\":{:?},\"radius_mm\":{},\"state\":{},\"recovery\":{},\"top_projection\":{projection},\"measurement_resolved\":false}}",cover.triangle,center,cover.radius,quote(state),quote(recovery)));
        }
    }
    let prefix = if let History::Explicit(entries) = &r.history {
        let decisions = entries
            .iter()
            .map(|e| e.json())
            .collect::<Vec<_>>()
            .join(",");
        let counts = source
            .captures
            .iter()
            .map(|c| {
                format!(
                    "{{\"capture\":{},\"completed_columns\":{}}}",
                    quote(c.id.as_str()),
                    source.samples.iter().filter(|s| s.capture == c.id).count()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let schema = if r.role.is_some() {
            "dmc2.spatial-observation-plan.v3"
        } else {
            "dmc2.spatial-observation-plan.v2"
        };
        format!(
            "{{\"schema\":\"{schema}\",\"acquisition_history\":{{\"decisions\":[{decisions}],\"included_captures\":[{counts}],\"completed_columns\":{},\"interpretation\":\"Explicit original top cycles with matching recorded start and plate/feed snapshots. Request reasons are annotations; numerical compatibility does not establish unchanged physical setup. Miss-only captures supply searched-column history, not fabricated surface contacts or resolved material coverage.\"}},",
            source.samples.len()
        )
    } else {
        String::from("{\"schema\":\"dmc2.spatial-observation-plan.v1\",")
    };

    let role_header = r
        .role
        .map(|v| {
            format!(
                "\"contact_role\":{},\"contact_role_interpretation\":{},",
                quote(v.name()),
                quote(v.description())
            )
        })
        .unwrap_or_default();
    let objective = quote(if r.role == Some(Role::Check) {
        "New XY columns in regions lacking independent support checks. Keep only predicted intersections inside a relevant unchecked top patch and the original descent range; retain other regions and unsupported candidates as unresolved. Prefer nearest prior search then grid index; previously searched columns are not selected again."
    } else {
        "One candidate per distinct XY cell intersecting a projected unsupported region. Prefer smallest distance to an original searched column, then grid index; triangle count does not weight priority. Original hit and miss columns are not selected again."
    });
    let required_volume = a
        .volume
        .as_ref()
        .map(|v| v.continuation(&a.surface.contacts))
        .unwrap_or_default();
    let json = format!(
        "{prefix}{role_header}\"state\":\"unreviewed-spatial-observation-proposals\",\"frame\":\"LinuxCNC machine-mm with original work translation retained\",\"frame_reference\":{},\"source_capture\":{},\"x_directions\":\"physical RIGHT / LinuxCNC -X; physical LEFT / LinuxCNC +X\",\"grid_origin_work_xy_mm\":{:?},\"sample_spacing_mm\":{},\"grid_location\":\"cell centres\",\"work_to_machine_translation_mm\":{:?},\"trigger_to_ball_mm\":{:?},\"retained_min_work_mm\":{:?},\"retained_max_work_mm\":{:?},\"initial_descent_floor_work_mm\":{},\"mounted_reach_floor_work_mm\":{},\"comparison_count\":{},\"selection_objective\":{objective},\"projection_formula\":\"region_work_xy = candidate_region_machine_xy - trigger_to_ball_xy - original_work_to_machine_xy; downward pretravel has no XY term\",\"selected_priority_order\":{:?},\"execution_order\":null,\"cells\":[{}],\"unresolved_material_regions\":[{}],\"interpretation\":\"Required geometry supplies XY investigation regions, never the stock outline, normal, contact height or new descent limit. Approach clearance, depth, feeds and travel bounds come from the selected original top capture. Cell centres can lie outside a region's projection by up to half the cell diagonal. New sampling retains missing spatial/height coverage, contact/miss conflicts, entry review and inaccessible surfaces as unresolved. No original trigger or contact role changed.\",\"closed_volume_coverage\":\"unresolved\"{required_volume},\"cam_ready\":false,\"machine_action_authorized\":false}}\n",
        quote(&a.candidate.frame),
        quote(r.capture.as_str()),
        selected.grid.origin,
        selected.grid.spacing,
        s.offset,
        a.surface.request.probe.mount,
        s.min,
        s.max,
        s.floor,
        s.reach_floor,
        selected.comparisons,
        selected.chosen,
        cells.join(","),
        regions.join(",")
    );
    Ok(Report { json, csv })
}
