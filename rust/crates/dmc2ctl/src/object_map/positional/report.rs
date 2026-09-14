//! Observable dimensions and per-trigger residuals; no stock solid is inferred.
use super::{
    super::{model::CaptureState, record::quote, Error},
    fit::{Fit, Sample},
    geometry::*,
    mesh::Mesh,
    request::{Request, Use},
};
pub struct Report {
    pub json: String,
    pub csv: String,
    pub centers: String,
    pub surfaces: String,
}
fn stats(values: &[f64]) -> String {
    if values.is_empty() {
        return "null".into();
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = if sorted.len() % 2 == 0 {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.
    } else {
        sorted[sorted.len() / 2]
    };
    let rms = values.iter().map(|v| v * v).sum::<f64>().sqrt() / (values.len() as f64).sqrt();
    format!("{{\"count\":{},\"median_mm\":{},\"min_mm\":{},\"max_mm\":{},\"rms_mm\":{},\"span_mm\":{}}}",values.len(),median,sorted[0],sorted[sorted.len()-1],rms,sorted[sorted.len()-1]-sorted[0])
}
fn median(v: &[f64]) -> Option<f64> {
    if v.is_empty() {
        None
    } else {
        let mut v = v.to_vec();
        v.sort_by(f64::total_cmp);
        Some((v[(v.len() - 1) / 2] + v[v.len() / 2]) / 2.)
    }
}
fn option(v: Option<f64>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "null".into())
}
pub fn build(mesh: &Mesh, samples: &[Sample], fit: &Fit, r: &Request) -> Result<Report, Error> {
    let mut csv_out=String::from("capture,sequence,use,capture_state,trigger_machine_x_mm,trigger_machine_y_mm,trigger_machine_z_mm,commanded_feed_mm_min,center_model_x_mm,center_model_y_mm,center_model_z_mm,nearest_triangle,nearest_model_x_mm,nearest_model_y_mm,nearest_model_z_mm,sphere_surface_residual_mm,huber_weight,facing,within_association_bound\n");
    let mut centers = String::new();
    let mut surfaces = String::new();
    let mut residuals = [Vec::new(), Vec::new(), Vec::new()];
    let mut faces: [Vec<f64>; 6] = std::array::from_fn(|_| Vec::new());
    for (s, o) in samples.iter().zip(&fit.observations) {
        let group = match s.usage {
            Use::Fit => 0,
            Use::Check => 1,
            _ => 2,
        };
        residuals[group].push(o.residual);
        let estimated = sub(o.center, scale(o.near.normal, r.probe.radius));
        let machine_center = fit.model_to_machine.point(o.center);
        let machine_surface = fit.model_to_machine.point(estimated);
        if !finite(machine_center) || !finite(machine_surface) {
            return Err(Error::Data(
                "Report coordinate overflows; no geometry was published.".into(),
            ));
        }
        centers.push_str(&format!("{}\n", csv(machine_center).replace(',', " ")));
        surfaces.push_str(&format!("{}\n", csv(machine_surface).replace(',', " ")));
        csv_out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            s.capture.as_str(),
            s.sequence,
            s.usage.name(),
            s.state.name(),
            csv(s.trigger),
            s.feed,
            csv(o.center),
            o.near.triangle,
            csv(o.near.point),
            o.residual,
            o.weight,
            o.facing,
            o.residual.abs() <= r.correspondence
        ));
        if let Use::Face(axis, positive) = s.usage {
            let sign = if positive { 1. } else { -1. };
            let approach = mv(fit.model_to_machine.inverse().r, s.approach);
            if sign * approach[axis] >= 0. {
                return Err(Error::Data(format!("Stock face {}:{} is labelled {} but the recorded approach points away from that outward normal. Correct the face assignment; no dimension was inferred.",s.capture.as_str(),s.sequence,s.usage.name())));
            }
            faces[axis * 2 + usize::from(positive)].push(o.center[axis] - sign * r.probe.radius);
        }
    }
    let face_json = (0..6)
        .map(|i| {
            let outward = if i % 2 == 1 { 1. } else { -1. };
            let nominal = if i % 2 == 1 {
                mesh.max[i / 2]
            } else {
                mesh.min[i / 2]
            };
            format!(
                "{{\"face\":{},\"surface_coordinate\":{},\"allowance_to_model_bound_mm\":{}}}",
                quote(Use::Face(i / 2, i % 2 == 1).name()),
                stats(&faces[i]),
                option(median(&faces[i]).map(|x| outward * (x - nominal)))
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let dimensions = (0..3)
        .map(|i| {
            option(
                median(&faces[2 * i])
                    .zip(median(&faces[2 * i + 1]))
                    .map(|(a, b)| b - a),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    for i in 0..3 {
        if median(&faces[2 * i])
            .zip(median(&faces[2 * i + 1]))
            .is_some_and(|(a, b)| a >= b)
        {
            return Err(Error::Data("Opposing stock faces are reversed or have nonpositive separation; inspect face assignments and probe correction.".into()));
        }
    }
    let partial = samples.iter().any(|s| s.state == CaptureState::Partial);
    let mismatched_checks = samples
        .iter()
        .zip(&fit.observations)
        .filter(|(s, o)| {
            s.usage == Use::Check && (!o.facing || o.residual.abs() > r.correspondence)
        })
        .count();
    let result=format!("{{\n\"schema\":\"dmc2.positional-analysis.v1\",\n\"state\":{},\n\"message\":{},\n\"cam_ready\":false,\n\"calibration_state\":{},\n\"model_role\":{},\n\"stl_mm_per_unit\":{},\n\"frame\":\"model-mm to LinuxCNC machine-mm\",\n\"model_to_machine\":{},\n\"iterations\":{},\n\"minimum_scaled_qr_pivot\":{},\n\"global_uniqueness_established\":false,\n\"uncertainty_mm\":null,\n\"contains_partial_capture\":{},\n\"fit_residuals\":{},\n\"independent_check_residuals\":{},\n\"independent_checks_outside_association\":{},\n\"other_residuals\":{},\n\"model_geometry\":{},\n\"stock_faces\":[{}],\n\"stock_face_separations_xyz_mm\":[{}],\n\"predicted_finished_model_span_mm\":{},\n\"unobserved_volume\":\"unknown\",\n\"reconstructed_stock\":null,\n\"correction_formula\":\"center_machine = original_trigger_machine + trigger_to_ball_mm - pretravel_mm * commanded_approach_unit; estimated_surface_model = inverse_pose(center_machine) - ball_radius_mm * nearest_surface_normal\",\n\"interpretation\":\"A local placement proposal. Normals and surface estimates depend on the STL and initial placement; signed residuals use local triangle winding, not a global solid-membership test. Huber weights retain every fitting row. Check rows do not affect the fit. Stock faces assume assigned planes parallel to the corresponding model axes; dimensions require both opposing faces. A model span is an ideal target prediction, not evidence of finished size. No offsets, CAM jobs, toolpaths or hardware states are changed.\"\n}}\n",quote(fit.outcome.name()),quote(fit.outcome.message()),quote(r.probe.calibration.name()),quote(&r.model_role),r.units,fit.model_to_machine.json(),fit.iterations,fit.min_scaled_pivot,partial,stats(&residuals[0]),stats(&residuals[1]),mismatched_checks,stats(&residuals[2]),mesh.json(),face_json,dimensions,json(sub(mesh.max,mesh.min)));
    // Reject arithmetic overflow in statistics instead of serializing invalid JSON.
    if result.contains(":inf") || result.contains(":NaN") || result.contains(":-inf") {
        return Err(Error::Data(
            "Report statistics overflowed; check units and measurements.".into(),
        ));
    }
    Ok(Report {
        json: result,
        csv: csv_out,
        centers,
        surfaces,
    })
}
