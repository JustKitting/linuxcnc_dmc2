use super::super::{
    material::Assessment,
    optimize::{Fitted, Stop},
};
use super::search::{Kind, ObjectiveData, Row};
use crate::object_map::{positional::geometry::*, record::quote};
pub struct Report {
    pub json: String,
    pub csv: String,
    pub history: String,
}
pub fn build(
    a: &Assessment,
    objective: &ObjectiveData<'_>,
    f: &Fitted<6>,
    before: &[Row],
    after: &[Row],
) -> Report {
    let stop = match f.stop {
        Stop::Clearance => "supported-constraints-met",
        Stop::Resolution => "search-resolution",
        Stop::Budget => "search-budget",
        Stop::FloatLimit => "numerical-subdivision-limit",
    };
    let mut csv=String::from("constraint,region,source_capture,source_sequence,sweep_sample,initial_slack_mm,candidate_slack_mm\n");
    for (b, c) in before.iter().zip(after) {
        let (capture, sequence) = match c.kind {
            Kind::EmptySweep => {
                let s = &a.surface.no_contact[c.source];
                (s.capture.as_str(), s.sequence)
            }
            _ => {
                let s = &a.surface.contacts[c.source];
                (s.capture.as_str(), s.sequence)
            }
        };
        csv.push_str(&format!(
            "{},{},{capture},{sequence},{},{},{}\n",
            c.kind.name(),
            c.region.map(|i| i.to_string()).unwrap_or_default(),
            c.sample.map(|i| i.to_string()).unwrap_or_default(),
            b.slack,
            c.slack
        ));
    }
    let mut history = String::from(
        "evaluation,dx_mm,dy_mm,dz_mm,roll_rad,pitch_rad,yaw_rad,minimum_constraint_slack_mm\n",
    );
    for (n, p, v) in &f.history {
        history.push_str(&format!("{n},{},{v}\n", p.map(|x| x.to_string()).join(",")));
    }
    let sweeps=objective.balls.iter().map(|b|format!("{{\"source\":{},\"part\":{},\"parts\":{},\"machine_center_mm\":{},\"path_cover_radius_mm\":{}}}",a.surface.no_contact[b.source].reference(),b.part,b.parts,json(b.center),b.radius)).collect::<Vec<_>>().join(",");
    let initial = before.iter().map(|r| r.slack).fold(f64::INFINITY, f64::min);
    let json=format!("{{\"schema\":\"dmc2.partial-placement.v1\",\"search_stop\":{},\"evaluations\":{},\"initial_minimum_constraint_slack_mm\":{initial},\"candidate_minimum_constraint_slack_mm\":{},\"remaining_objective_upper_mm\":{},\"initial_model_to_machine\":{},\"candidate_model_to_machine\":{},\"reserved_patch_comparisons\":{},\"reserved_winding_terms\":{},\"finite_sweep_covers\":[{sweeps}],\"objective\":\"Maximize minimum slack of every initially checked measured-plane separation and finite lateral support margin, plus complete finite clear-sweep bounds against the unchanged required material. Zero slack is the inherited threshold; no weights or tolerances are invented.\",\"interpretation\":\"Local refinement retains each original supported comparison. It cannot improve by dropping a comparison outside its finite support. Signed-distance cover bounds include wholly contained empty sweeps. All final material comparisons are recalculated, including previously unsupported regions. Unknown volume remains unknown; meeting these constraints does not release the material pipeline. Bounds are numerical, not physical or formal arithmetic certificates.\",\"placement_accepted\":false,\"cam_ready\":false}}",quote(stop),f.evaluations,f.clearance,f.upper,a.candidate.pose.json(),super::search::pose(a.candidate.pose,f.at).json(),objective.reserved_patches,objective.reserved_winding);
    Report { json, csv, history }
}
