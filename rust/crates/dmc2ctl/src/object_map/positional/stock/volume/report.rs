use super::super::super::{cover::Sample, geometry::*, mesh::solid::Solid};
use super::{
    request::Request,
    search::{self, Fitted},
};
use crate::object_map::{Error, record::quote};
pub struct Report {
    pub json: String,
    pub csv: String,
    pub history: String,
}
pub fn build(
    stock: &Solid<'_>,
    samples: &[Sample],
    f: &Fitted,
    r: &Request,
) -> Result<Report, Error> {
    let pose = search::pose(f.at);
    let mut rows = String::from(
        "cover_sample,source_triangle,model_x_mm,model_y_mm,model_z_mm,machine_x_mm,machine_y_mm,machine_z_mm,cover_radius_mm,inside_distance_mm,clearance_lower_mm,clearance_upper_mm,clearance_deficit_lower_mm,clearance_deficit_upper_mm,nearest_stock_triangle,winding\n",
    );
    let (mut worst_lower, mut worst_upper, mut deficits) = (0_f64, 0_f64, 0usize);
    for (i, s) in samples.iter().enumerate() {
        let p = pose.point(s.center);
        let d = stock.distance(p)?;
        let lower = d.inward - s.radius - r.allowance;
        let upper = d.inward - r.allowance;
        let dl = (r.margin - upper).max(0.);
        let du = (r.margin - lower).max(0.);
        if ![lower, upper, dl, du].iter().all(|v| v.is_finite()) {
            return Err(Error::Data("Volume deficit arithmetic overflowed. Inspect units, clearance and allowance before another analysis.".into()));
        }
        worst_lower = worst_lower.max(dl);
        worst_upper = worst_upper.max(du);
        deficits += usize::from(dl > 0.);
        rows.push_str(&format!(
            "{i},{},{},{},{},{},{lower},{upper},{dl},{du},{},{}\n",
            s.triangle,
            csv(s.center),
            csv(p),
            s.radius,
            d.inward,
            d.triangle,
            d.winding.map_or(String::new(), |w| w.to_string())
        ));
    }
    let mut history = String::from(
        "evaluation,model_origin_x_mm,model_origin_y_mm,model_origin_z_mm,roll_deg,pitch_deg,yaw_deg,clearance_lower_mm\n",
    );
    for (n, p, c) in &f.history {
        history.push_str(&format!(
            "{n},{},{},{},{},{},{},{c}\n",
            p[0],
            p[1],
            p[2],
            p[3].to_degrees(),
            p[4].to_degrees(),
            p[5].to_degrees()
        ));
    }
    let (state, message) = search::description(f.stop);
    let json = format!(
        "{{\"schema\":\"dmc2.volume-placement.v1\",\"state\":{},\"message\":{},\"required_geometry_role\":\"operation-retained-material\",\"stock_occupancy_model\":{},\"model_to_machine\":{},\"covered_samples\":{},\"evaluations\":{},\"requested_clearance_mm\":{},\"surface_allowance_mm\":{},\"candidate_clearance_lower_mm\":{},\"remaining_objective_upper_mm\":{},\"worst_clearance_deficit_lower_mm\":{worst_lower},\"worst_clearance_deficit_upper_mm\":{worst_upper},\"samples_with_clearance_deficit\":{deficits},\"covered_required_surface_within_stock_model\":{},\"stock_geometry_checks\":{{\"closed_consistent_outward_shell\":true,\"single_connected_shell\":true,\"vertex_links_checked\":{},\"intersecting_aabb_triangle_pairs_checked\":{},\"hierarchy_visits\":{},\"self_intersections_detected\":false}},\"objective\":\"Maximize the minimum inside signed distance minus triangle-cover radius minus the explicit surface allowance. Excess stock is not a shape-matching residual. Full required triangles and source IDs are preserved.\",\"interpretation\":\"The enclosed material model explicitly assumes one boundary shell without hidden cavities. Its geometry checks do not establish physical occupancy or measurement uncertainty. Full spatial cover balls bound source triangles, not just vertices. Clearance and remaining-search bounds use floating-point geometry, not a formal arithmetic or physical certificate. Six pose parameters use translation then Rz(yaw) Ry(pitch) Rx(roll); only requested ranges are searched. Setup/workholding/tool-access constraints are not inferred from this geometric fit.\",\"placement_accepted\":false,\"cam_ready\":false}}",
        quote(state),
        quote(message),
        quote(r.occupancy.name()),
        pose.json(),
        samples.len(),
        f.evaluations,
        r.margin,
        r.allowance,
        f.clearance,
        f.upper,
        f.clearance >= r.margin,
        stock.vertices,
        stock.triangle_pairs,
        stock.topology_visits
    );
    Ok(Report {
        json,
        csv: rows,
        history,
    })
}
