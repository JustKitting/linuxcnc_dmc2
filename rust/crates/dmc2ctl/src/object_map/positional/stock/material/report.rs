use super::super::super::{cover, geometry::*, mesh::Triangle, probe::Sample, Error};
use super::{
    empty,
    query::{Checks, Region, State},
    request::{EmptySpace, Request},
};
use crate::object_map::record::quote;
pub struct Report {
    pub json: String,
    pub csv: String,
    pub needs: String,
}
fn reference(s: &Sample) -> String {
    format!(
        "{{\"capture\":{},\"sequence\":{}}}",
        quote(s.capture.as_str()),
        s.sequence
    )
}
pub fn build(
    cover: &[cover::Sample],
    samples: &[Sample],
    regions: &[Region],
    triangles: &[Triangle],
    pose: Pose,
    r: &Request,
    misses: &[super::surface::no_contact::Sweep],
    volume: Option<&super::volume::Assessment>,
) -> Result<Report, Error> {
    let version = r.empty.version();
    let (empty_header, empty_tail) = match r.empty {
        EmptySpace::Legacy => (String::new(), String::new()),
        EmptySpace::RetainedSweeps { .. } | EmptySpace::RequiredVolume { .. } => (
            format!(",{}", empty::COLUMNS),
            ",".repeat(empty::COLUMNS.split(',').count()),
        ),
    };
    let mut csv_rows = format!("region,source_triangle,machine_x_mm,machine_y_mm,machine_z_mm,cover_radius_mm,state,patch_capture,patch_sequence,checks,signed_outside_distance_mm,local_clearance_lower_mm,local_clearance_upper_mm,clearance_deficit_lower_mm,clearance_deficit_upper_mm{empty_header}\n");
    let mut rows = Vec::new();
    let mut needs = Vec::new();
    let mut worst_lower = 0_f64;
    let mut worst_upper = 0_f64;
    let mut counts = std::collections::BTreeMap::new();
    for (i, (sample, region)) in cover.iter().zip(regions).enumerate() {
        let p = pose.point(sample.center);
        let (state, message) = region.state.description();
        *counts.entry(state).or_insert(0usize) += 1;
        let prefix = format!(
            "{i},{},{},{},{}",
            sample.triangle,
            csv(p),
            sample.radius,
            state
        );
        let mut comparisons = Vec::new();
        for c in &region.comparisons {
            let deficit_lower = (r.clearance - c.upper).max(0.);
            let deficit_upper = (r.clearance - c.lower).max(0.);
            if !deficit_lower.is_finite() || !deficit_upper.is_finite() {
                return Err(Error::Data("Material deficit arithmetic overflowed. Inspect request clearance and source units.".into()));
            }
            if c.checks == Checks::Within {
                worst_lower = worst_lower.max(deficit_lower);
                worst_upper = worst_upper.max(deficit_upper);
            }
            let source = &samples[c.source];
            csv_rows.push_str(&format!(
                "{prefix},{},{},{},{},{},{},{},{}{empty_tail}\n",
                source.capture.as_str(),
                source.sequence,
                c.checks.name(),
                c.distance,
                c.lower,
                c.upper,
                deficit_lower,
                deficit_upper
            ));
            let checks = c
                .check_sources
                .iter()
                .map(|j| reference(&samples[*j]))
                .collect::<Vec<_>>()
                .join(",");
            comparisons.push(format!("{{\"source\":{},\"independent_checks\":[{checks}],\"check_state\":{},\"signed_outside_distance_mm\":{},\"clearance_lower_mm\":{},\"clearance_upper_mm\":{},\"clearance_deficit_lower_mm\":{deficit_lower},\"clearance_deficit_upper_mm\":{deficit_upper},\"projected_surface_machine_mm\":{},\"estimated_outward_normal\":{}}}",reference(source),quote(c.checks.name()),c.distance,c.lower,c.upper,json(c.projection),json(c.normal)));
        }
        if comparisons.is_empty() && region.no_contact.is_empty() {
            csv_rows.push_str(&format!("{prefix},,,,,,,,{empty_tail}\n"));
        }
        for o in &region.no_contact {
            csv_rows.push_str(&format!("{prefix},,,,,,,,,{}\n", o.csv()));
        }
        let (empty_detail, empty_need) = match r.empty {
            EmptySpace::Legacy => (String::new(), String::new()),
            EmptySpace::RetainedSweeps { .. } | EmptySpace::RequiredVolume { .. } => (
                format!(",\"fragment_model_mm\":{:?},\"fragment_machine_mm\":{:?},\"no_contact_overlaps\":[{}]", sample.vertices,sample.vertices.map(|p|pose.point(p)),region.no_contact.iter().map(|o|o.json(samples)).collect::<Vec<_>>().join(",")),
                format!(",\"no_contact_sources\":[{}]",region.no_contact.iter().map(|o|format!("{{\"source\":{},\"relation\":\"{}\"}}",o.source.sweep.reference(),o.relation())).collect::<Vec<_>>().join(",")),
            ),
        };
        let source_tri = sample.triangle;
        let detail = format!("{{\"region\":{i},\"source_triangle\":{source_tri},\"model_center_mm\":{},\"machine_center_mm\":{},\"cover_radius_mm\":{},\"state\":{},\"comparisons\":[{}]{empty_detail}}}",json(sample.center),json(p),sample.radius,quote(state),comparisons.join(","));
        rows.push(detail);
        if region.state != State::LocallyInward {
            // The required facet normal indicates a design-side region of
            // interest, never an observed stock normal or approved approach.
            let n = mv(pose.r, triangles[source_tri].n);
            needs.push(format!("{{\"kind\":{},\"message\":{},\"region\":{i},\"source_triangle\":{source_tri},\"region_center_machine_mm\":{},\"region_radius_mm\":{},\"required_facet_outward_normal\":{},\"stock_normal_inferred_from_design\":false,\"machine_action_authorized\":false{empty_need}}}",quote(state),quote(message),json(p),sample.radius,json(n)));
        }
    }
    if let Some(volume) = volume {
        needs.extend(volume.needs(samples));
    }
    needs.push("{\"kind\":\"closed-material-coverage-unresolved\",\"message\":\"Local comparisons cannot establish enclosed material, underside coverage or cavities. Complete and review the stock boundary/support model before containment or cutting.\",\"machine_action_authorized\":false}".into());
    let needs = format!("{{\"schema\":\"dmc2.candidate-measurement-needs.{version}\",\"candidate_analysis\":{},\"surface_analysis\":{},\"needs\":[{}],\"execution\":\"unplanned-observation-regions\",\"machine_commands_issued\":false}}\n",quote(r.candidate.as_str()),quote(r.surface.as_str()),needs.join(","));
    let counts = counts
        .iter()
        .map(|(k, v)| format!("{}:{v}", quote(k)))
        .collect::<Vec<_>>()
        .join(",");
    let empty_model = match r.empty {
        EmptySpace::Legacy => String::new(),
        EmptySpace::RetainedSweeps { .. } | EmptySpace::RequiredVolume { .. } => format!(",\"no_contact_model\":{{\"retained_sweeps\":[{}],\"formula\":\"signed_separation = distance(actual transformed triangle fragment, finite retained centre path) - eroded_ball_radius\",\"interpretation\":\"Negative separation establishes a required-fragment intersection under the retained probe/error model; zero is its boundary. Original contact conflicts remain explicit. No intersection does not establish material presence, closed stock, unseen cavities or physical calibration. A covering ball is not used as the required triangle.\"}}",misses.iter().map(|s|s.json()).collect::<Vec<_>>().join(",")),
    };
    let empty_model = empty_model
        + &volume
            .map(|v| format!(",\"required_volume_assessment\":{}", v.json(samples)))
            .unwrap_or_default();
    let json=format!("{{\"schema\":\"dmc2.material-check.{version}\",\"state\":\"material-coverage-unresolved\",\"frame\":\"LinuxCNC machine-mm\",\"model_to_machine\":{},\"region_counts\":{{{counts}}},\"covered_regions\":{},\"checked_local_deficit_lower_max_mm\":{worst_lower},\"checked_local_deficit_upper_max_mm\":{worst_upper},\"regions\":[{}],\"measurement_needs\":{},\"solid_stock\":null,\"unmeasured_volume\":\"unknown\",\"cam_ready\":false{empty_model},\"interpretation\":\"Full 3D subtriangle covers are compared only inside measured local planar support and the explicit normal band. Minimum local clearance bounds are [-d-radius-allowance,-d+allowance], where d=(candidate-center minus surface-point) dot outward-normal. A shortage is a local model/clearance result; inwardness does not establish material or a closed volume. All eligible local comparisons and independent check conflicts remain retained. Region centers and required-facet normals describe data needs, not probe endpoints, measured stock normals or approved hardware actions.\"}}\n",pose.json(),regions.len(),rows.join(","),needs);
    Ok(Report {
        json,
        csv: csv_rows,
        needs,
    })
}
