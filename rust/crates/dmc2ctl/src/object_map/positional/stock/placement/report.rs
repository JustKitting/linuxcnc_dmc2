use super::super::super::{Error, geometry::*};
use super::{
    cover::Sample,
    polygon::Polygon,
    request::Request,
    search::{self, Fitted},
};
use crate::object_map::record::quote;
pub struct Report {
    pub json: String,
    pub csv: String,
    pub history: String,
}
pub fn build(poly: &Polygon, samples: &[Sample], f: &Fitted, r: &Request) -> Result<Report, Error> {
    let pose = search::pose(f.at, r.z);
    let mut csv_rows = String::from(
        "cover_sample,source_triangle,model_x_mm,model_y_mm,model_z_mm,machine_x_mm,machine_y_mm,machine_z_mm,cover_radius_mm,signed_outside_distance_mm,clearance_lower_mm,clearance_upper_mm,clearance_deficit_lower_mm,clearance_deficit_upper_mm,nearest_outline_segment\n",
    );
    let mut worst_lower = 0_f64;
    let mut worst_upper = 0_f64;
    let mut deficits = 0usize;
    for (i, s) in samples.iter().enumerate() {
        let p = pose.point(s.center);
        let (d, edge) = poly.distance(p)?;
        let lower = -d - s.radius;
        let upper = -d;
        let deficit_lower = (r.margin - upper).max(0.);
        let deficit_upper = (r.margin - lower).max(0.);
        if ![lower, upper, deficit_lower, deficit_upper]
            .iter()
            .all(|v| v.is_finite())
        {
            return Err(Error::Data(
                "Footprint deficit arithmetic overflowed; inspect request units and clearance."
                    .into(),
            ));
        }
        worst_lower = worst_lower.max(deficit_lower);
        worst_upper = worst_upper.max(deficit_upper);
        deficits += usize::from(deficit_lower > 0.);
        csv_rows.push_str(&format!(
            "{i},{},{},{},{},{d},{lower},{upper},{deficit_lower},{deficit_upper},{edge}\n",
            s.triangle,
            csv(s.center),
            csv(p),
            s.radius
        ));
    }
    let mut history =
        String::from("evaluation,model_origin_x_mm,model_origin_y_mm,yaw_deg,clearance_lower_mm\n");
    for (n, p, c) in &f.history {
        history.push_str(&format!(
            "{n},{},{},{},{c}\n",
            p[0],
            p[1],
            p[2].to_degrees()
        ));
    }
    let polygon = poly
        .vertices
        .iter()
        .map(|p| json(*p))
        .collect::<Vec<_>>()
        .join(",");
    let (state, message) = search::description(f.stop);
    let result = format!(
        "{{\"schema\":\"dmc2.footprint-placement.v1\",\"state\":{},\"message\":{},\"required_geometry_role\":\"operation-retained-material\",\"model_to_machine\":{},\"estimated_outline_machine_mm\":[{polygon}],\"covered_samples\":{},\"evaluations\":{},\"requested_clearance_mm\":{},\"candidate_clearance_lower_mm\":{},\"remaining_objective_upper_mm\":{},\"worst_clearance_deficit_lower_mm\":{worst_lower},\"worst_clearance_deficit_upper_mm\":{worst_upper},\"samples_with_clearance_deficit\":{deficits},\"horizontal_clearance_within_model\":{},\"three_dimensional_containment\":\"unresolved\",\"unmeasured_volume\":\"unknown\",\"cam_ready\":false,\"objective\":\"Find an allowed translation/yaw with min over triangle cover samples of [inside signed distance minus cover radius] at least the requested clearance; all local deficits remain reported. Excess stock is not a fitting residual.\",\"interpretation\":\"The source outline retains its local interpolation and vertical-side probe correction assumptions. This projection does not establish top, bottom, taper, cavities, fixtures, tool clearance or 3D material. Cover balls bound complete projected triangles, not just vertices; bounds are numerical floating-point calculations for this polygon model, not physical accuracy. Smaller cover radius reduces conservatism. Mesh dimensions and source triangle identities are preserved.\"}}",
        quote(state),
        quote(message),
        pose.json(),
        samples.len(),
        f.evaluations,
        r.margin,
        f.clearance,
        f.upper,
        f.clearance >= r.margin
    );
    Ok(Report {
        json: result,
        csv: csv_rows,
        history,
    })
}
