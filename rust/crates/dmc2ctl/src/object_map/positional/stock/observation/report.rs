use super::super::{super::geometry::*, report::reference};
use super::{
    material::{self, query::State},
    select::Selection,
};
use crate::object_map::{record::quote, Error};
pub struct Report {
    pub json: String,
    pub csv: String,
}
pub fn build(a: &material::Assessment, selected: &Selection) -> Result<Report, Error> {
    let samples = &a.surface.contacts;
    let mut proposals = Vec::new();
    for (i, c) in selected.candidates.iter().enumerate() {
        let source = &samples[c.sources[0]];
        let refs = c
            .sources
            .iter()
            .map(|&j| reference(&samples[j]))
            .collect::<Vec<_>>()
            .join(",");
        let status = match &c.proposal {
            Err(e) => format!(
                "{{\"state\":\"original-approach-unavailable\",\"recovery\":{}}}",
                quote(e)
            ),
            Ok(p) => {
                let run = selected.runs.get(source.capture.as_str()).and_then(|r| r.as_ref().ok()).ok_or_else(|| Error::Data("A selected observation lost its retained acquisition settings. Preserve its analysis and recalculate the observation request.".into()))?;
                let s = &run.settings;
                // Runtime sequence/sample assignment is deliberately absent.
                // Remaining fields use the actual acquisition request contract.
                let fields = p
                    .request
                    .values(s, 0, 0)
                    .into_iter()
                    .filter(|(key, _)| !matches!(*key, "sequence" | "sample"))
                    .map(|(key, v)| format!("{}:{v}", quote(key)))
                    .collect::<Vec<_>>()
                    .join(",");
                let start_machine = add(p.start, s.offset);
                let target_machine = add(p.request.target, s.offset);
                if !finite(start_machine) || !finite(target_machine) {
                    return Err(Error::Data("Observation machine-coordinate translation overflowed. Inspect the original frame and units; no substitute target was emitted.".into()));
                }
                format!(
                    "{{\"state\":\"bounded-original-request-proposal\",\"entry\":{},\"entry_requirement\":{},\"entry_work_mm\":{:?},\"entry_machine_mm\":{:?},\"original_fine_trigger_machine_mm\":{:?},\"original_returned_work_mm\":{:?},\"work_to_machine_translation_mm\":{:?},\"target_machine_mm\":{:?},\"retained_min_work_mm\":{:?},\"retained_max_work_mm\":{:?},\"initial_descent_floor_work_mm\":{},\"mounted_reach_floor_work_mm\":{},\"request_contract_fields\":{{{fields}}},\"runtime_sequence\":null,\"new_trigger\":null,\"entry_path_supplied\":false,\"machine_action_authorized\":false}}",
                    quote(p.entry.name()),
                    quote(p.entry.recovery()),
                    p.start,
                    start_machine,
                    p.original_trigger,
                    p.original_returned,
                    s.offset,
                    target_machine,
                    s.min,
                    s.max,
                    s.floor,
                    s.reach_floor
                )
            }
        };
        let needs = c
            .needs
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(",");
        proposals.push(format!("{{\"proposal\":{i},\"source_contacts\":[{refs}],\"selected_priority\":{},\"addresses_patch_requirements\":[{needs}],\"future_capture_role\":\"independent-check\",\"original_contact_roles_changed\":false,\"acquisition\":{status}}}", selected.chosen.iter().position(|&j| j == i).map(|n| n.to_string()).unwrap_or_else(|| "null".into())));
    }
    let needs = selected.needs.iter().enumerate().map(|(i, n)| {
        let regions = n.regions.iter().map(usize::to_string).collect::<Vec<_>>().join(",");
        let planned = selected.chosen.iter().any(|&c| selected.candidates[c].needs.contains(&i));
        format!("{{\"requirement\":{i},\"kind\":{},\"patch_source\":{},\"material_regions\":[{regions}],\"repeat_observation_selected\":{planned},\"measurement_resolved\":false}}",quote(n.kind.name()),reference(&samples[n.seed]))
    }).collect::<Vec<_>>().join(",");
    let mut csv = String::from(
        "region,source_triangle,x_machine_mm,y_machine_mm,z_machine_mm,cover_radius_mm,material_state,patch_requirements,selected_repeat_proposals\n",
    );
    let mut regions = Vec::new();
    for (i, (cover, r)) in a.covers.iter().zip(&a.regions).enumerate() {
        let center = a.candidate.pose.point(cover.center);
        let ids = selected
            .needs
            .iter()
            .enumerate()
            .filter_map(|(j, n)| n.regions.contains(&i).then_some(j))
            .collect::<Vec<_>>();
        let observations = selected
            .chosen
            .iter()
            .filter(|&&c| ids.iter().any(|n| selected.candidates[c].needs.contains(n)))
            .copied()
            .collect::<Vec<_>>();
        let (state, recovery) = r.state.description();
        csv.push_str(&format!(
            "{i},{},{},{},{},{},{},{},{}\n",
            cover.triangle,
            center[0],
            center[1],
            center[2],
            cover.radius,
            state,
            ids.len(),
            observations.len()
        ));
        if r.state != State::LocallyInward {
            regions.push(format!("{{\"region\":{i},\"source_triangle\":{},\"center_machine_mm\":{:?},\"radius_mm\":{},\"state\":{},\"recovery\":{},\"patch_requirements\":{:?},\"selected_repeat_proposals\":{:?},\"measurement_resolved\":false}}",cover.triangle,center,cover.radius,quote(state),quote(recovery),ids,observations));
        }
    }
    let required_volume = a
        .volume
        .as_ref()
        .map(|v| v.continuation(samples))
        .unwrap_or_default();
    let json = format!(
        "{{\"schema\":\"dmc2.observation-plan.v1\",\"state\":\"unreviewed-observation-proposals\",\"frame\":\"LinuxCNC machine-mm with each original work translation retained\",\"frame_reference\":{},\"x_directions\":\"physical RIGHT / LinuxCNC -X; physical LEFT / LinuxCNC +X\",\"selection_objective\":\"Greedy coverage of distinct patch check/shortage requirements using original inward probing requests; deterministic source-order ties. Required triangle count does not weight selection. This is not a global minimum or an execution order.\",\"selected_priority_order\":{:?},\"execution_order\":null,\"patch_requirements\":[{needs}],\"unplanned_patch_requirements\":{:?},\"candidates\":[{}],\"unresolved_material_regions\":[{}],\"closed_volume_coverage\":\"unresolved\"{required_volume},\"interpretation\":\"Proposals retain prior exact approaches and settings, not current clearance, probe installation or operator authorization. A repeat produces a new independent observation only after exact trigger capture/readback. It cannot create missing spatial support, determine an unobserved normal, fill an unknown volume or clear a material shortage by itself. Unavailable approaches and computational coverage issues remain explicit. No design normal defines a probing direction and no old contact becomes a new check.\",\"cam_ready\":false,\"machine_action_authorized\":false}}\n",
        quote(&a.candidate.frame),
        selected.chosen,
        selected.pending,
        proposals.join(","),
        regions.join(",")
    );
    Ok(Report { json, csv })
}
