//! One adaptive iteration: noisy stock estimate -> placement -> next acquisition.
mod acquire;
pub(in crate::object_map) mod continuation;
mod model;
mod posterior;
mod random;
pub(in crate::object_map) mod request;
mod required;
mod search;
use super::surface;
use crate::{
    object_map::{
        model::{DesignFormat, Id},
        positional::{cover, folder, geometry::*, mesh::Mesh, pose_record},
        record,
        store::{read, save, Store},
        Error,
    },
    probe_data::{
        mapper_settings::{Mode, Settings},
        top_followup::{Plan, Role, Rows},
    },
};
use std::path::Path;

pub fn prepare(
    store: &Store,
    object: &Id,
    setup: &Id,
    design: &Id,
    capture: &Id,
) -> Result<String, Error> {
    if !store
        .designs(object)?
        .iter()
        .any(|d| d.id == *design && d.format == DesignFormat::Stl)
    {
        return Err(Error::Input(
            "Attach the unchanged required operation STL before preparing an adaptive cycle."
                .into(),
        ));
    }
    if !store
        .captures(object, setup)?
        .iter()
        .any(|c| c.id == *capture)
    {
        return Err(Error::Input(
            "Select a retained acquisition profile capture from this setup.".into(),
        ));
    }
    let fields = request::KEYS
        .iter()
        .map(|&k| {
            (
                k,
                match k {
                    "design" => design.as_str(),
                    "source_capture" => capture.as_str(),
                    _ => "REQUIRED",
                },
            )
        })
        .collect::<Vec<_>>();
    String::from_utf8(record::encode(request::SCHEMA, &fields, &[])?)
        .map_err(|e| Error::Data(e.to_string()))
}
enum Outcome {
    ModelReady,
    ModelUnsettled,
    Acquire,
    Shortage,
    Access,
}
impl Outcome {
    fn description(&self) -> (&'static str, &'static str) {
        match self {
        Self::ModelReady=>("placement-estimate-ready-for-cam-review","The current noisy model predicts the requested material fits at this placement. Retain its uncertainty and independent residuals in CAM review."),
        Self::ModelUnsettled=>("stock-posterior-not-converged","The noisy stock calculation has not converged within the requested numerical tolerance/budget. Inspect retained model and residuals, adjust those numerical settings or the measurement model in a new request, and recalculate. Cutting remains paused."),
        Self::Acquire=>("needs-new-measurements","The current estimate needs further measurements. Review the generated top/side probe plan, acquire its exact ledger, and use Update fit from new measurements with this cycle and the new capture."),
        Self::Shortage=>("insufficient-material-at-candidate","Retained empty-space evidence intersects this candidate. Cutting is paused. The new observation plan targets evidence for another placement or a revised stock estimate."),
        Self::Access=>("measurement-access-unresolved","The candidate sampling budget produced no measurement within the declared acquisition envelope. Review the explicit profile/access limits or increase sampling; no unmeasured volume was certified."),
    }
    }
}
pub fn run(store: &Store, object: &Id, setup: &Id, id: &Id, input: &Path) -> Result<String, Error> {
    let output = folder(store, object, setup, id)?;
    if output.exists() {
        return Err(Error::Storage(
            "This adaptive analysis ID exists. Select a new ID to preserve the source cycle."
                .into(),
        ));
    }
    let raw = read(input)?;
    let r = request::Request::read(&raw)?;
    let source = surface::load(store, object, setup, &r.surface)?;
    if !matches!(
        source.request.no_contact_sources,
        surface::request::NoContactSources::Explicit(_)
    ) {
        return Err(Error::Input("Adaptive mapping requires a V3 surface revision with explicit retained miss-capture decisions. Prepare that revision from the original captures so an all-miss acquisition can contribute empty-space evidence.".into()));
    }
    let designs = store.designs(object)?;
    let design=designs.iter().find(|d|d.id==r.design && d.format==DesignFormat::Stl).ok_or_else(||Error::Input("The required STL revision is missing. Attach the exact design and correct the adaptive request.".into()))?;
    let mesh = Mesh::read(&design.raw, r.units)?;
    mesh.fitting_geometry()?;
    let covers = cover::build(mesh.triangles(), r.radius, r.covers, cover::Metric::Spatial)?;
    let captures = store.captures(object, setup)?;
    let profile=captures.iter().find(|c|c.id==r.capture).ok_or_else(||Error::Input("The acquisition profile capture is missing. Import its original settings/companions or select the intended profile.".into()))?;
    let mut start=profile.capture.records.first().ok_or_else(||Error::Data("The acquisition profile has no starting reference. Select an intact capture with its companions.".into()))?.clone();
    // A new directed plan uses the common follow-up executor. Its source
    // profile supplies frame/clearance/feed data, never target measurements.
    start.insert("mode".into(), (Mode::TopFollowup as u8).to_string());
    for (key, value) in ["x", "y", "z"].into_iter().zip(r.acquisition_start) {
        start.insert(key.into(), value.to_string());
    }
    start.insert("drop".into(), r.acquisition_drop.to_string());
    let snapshot = |kind: &str| -> Result<String, Error> {
        let bytes=profile.context.snapshots().find(|(k,_)|*k==kind).and_then(|(_,b)|b).ok_or_else(||Error::Data(format!("The acquisition profile lacks its {kind} snapshot. Import the intact companion before exporting.")))?;
        String::from_utf8(bytes.to_vec())
            .map_err(|e| Error::Data(format!("Acquisition {kind} snapshot is not UTF-8: {e}.")))
    };
    let mut plan = Plan {
        source: [
            object.as_str(),
            setup.as_str(),
            id.as_str(),
            r.capture.as_str(),
        ]
        .map(String::from),
        start,
        plate: snapshot("plate")?,
        feeds: snapshot("feeds")?,
        rows: Rows::Directed(Vec::new()),
        role: Some(Role::Fit),
    };
    let s = Settings::read(
        &plan.start,
        &crate::probe_data::mapper_settings::data(
            &plan.plate,
            "DMC2_PLATE_ENVELOPE_V1",
            crate::probe_data::mapper_settings::PLATE_FIELDS,
        )
        .map_err(Error::Data)?,
        &crate::probe_data::mapper_settings::policy(&plan.feeds).map_err(Error::Data)?,
        None,
    )
    .map_err(Error::Data)?;
    if (s.radius - source.request.probe.radius).abs()
        > f64::EPSILON * s.radius.max(source.request.probe.radius)
    {
        return Err(Error::Input("The selected acquisition profile and surface analysis use different probe radii. Select or prepare a profile for the installed probe; no diameter was substituted.".into()));
    }
    let mut random = random::Random(r.seed);
    let model = model::Model::fit(&source, &r, &mut random)?;
    let required = required::Required::build(&mesh, &covers, &r, &mut random)?;
    let fitted = search::run(&model, &required, &source.no_contact, &r, &mut random)?;
    let selection = acquire::run(
        &model,
        &fitted,
        &s,
        source.request.probe.mount,
        &r,
        &mut random,
    )?;
    let checked = !model.checks.is_empty()
        && model.checks.iter().all(|(_, e, v)| {
            e.abs() <= r.confidence * (v + r.contact_sigma * r.contact_sigma).sqrt()
        });
    let checked = checked
        && model
            .empty_checks
            .iter()
            .all(|(_, _, e, v)| *e <= r.confidence * (v + r.empty_sigma * r.empty_sigma).sqrt());
    let shortage = !fitted.best.empty_overlaps.is_empty();
    let outcome = if shortage {
        Outcome::Shortage
    } else if !model.posterior_converged {
        Outcome::ModelUnsettled
    } else if fitted.best.worst == 0. && checked && model.posterior_converged {
        Outcome::ModelReady
    } else if selection.rows.is_empty() {
        Outcome::Access
    } else {
        Outcome::Acquire
    };
    plan.role = Some(if fitted.best.worst == 0. {
        Role::Check
    } else {
        Role::Fit
    });
    plan.rows = Rows::Directed(selection.rows.iter().map(|row| row.request).collect());
    let program = if selection.rows.is_empty() {
        None
    } else {
        let program = plan.program().map_err(Error::Data)?;
        Plan::from_program(&program).map_err(Error::Data)?;
        Some(program)
    };
    let pose = search::pose(fitted.best.at).validate()?;
    let transformed = mesh.transformed_stl(pose)?;
    let (state, message) = outcome.description();
    let ready = matches!(outcome, Outcome::ModelReady);
    let observations=selection.rows.iter().enumerate().map(|(i,row)|format!("{{\"row\":{i},\"phase\":{},\"approach_work_xy_mm\":[{},{}],\"target_work_mm\":{},\"predicted_ball_center_machine_mm\":{},\"information_gain\":{},\"placement_target_machine_mm\":{},\"predicted_contact_bracketed\":{}}}",row.request.phase as u8,row.request.approach[0],row.request.approach[1],json(row.request.target),json(row.predicted_center),row.gain,json(row.target),row.bracketed)).collect::<Vec<_>>().join(",");
    let checks=model.checks.iter().map(|(i,e,v)|format!("{{\"capture\":{},\"sequence\":{},\"residual_mm\":{e},\"latent_variance_mm2\":{v}}}",record::quote(source.contacts[*i].capture.as_str()),source.contacts[*i].sequence)).collect::<Vec<_>>().join(",");
    let report=format!("{{\"schema\":\"dmc2.adaptive-stock-cycle.v1\",\"state\":{},\"message\":{},\"surface_analysis\":{},\"model_to_machine\":{},\"rms_predicted_deficit_mm\":{},\"worst_predicted_deficit_mm\":{},\"evaluations\":{},\"contact_observations\":{},\"normal_observations\":{},\"empty_space_observations\":{},\"posterior_iterations\":{},\"posterior_converged\":{},\"model_discrepancy_variance_mm2\":{},\"independent_checks\":[{checks}],\"observation_candidates\":{},\"outside_acquisition_envelope\":{},\"next_observations\":[{observations}],\"advance_to_cam_review\":{ready},\"advance_to_cutting\":false,\"model\":\"Noisy signed-distance estimate using Bayesian random Fourier features, noisy estimated normal derivatives and censored logistic empty-path observations; uncertainty is conditional on the explicit model/noise settings, not a physical guarantee. Probe contact likelihood conditions the predicted information gain. Area-weighted required surface coverage and stochastic interior samples assess the proposed placement. Every retained finite empty path is also compared against the required solid and clearance. Stock occupancy remains a model prediction; fixtures are not represented by this calculation.\",\"execution\":\"Review adaptive-probe.ngc through standard File Open/Run. Import the fresh exact ledger and companions, then use Update fit from new measurements to retain them in a new surface revision and recalculate placement and acquisition.\"}}",record::quote(state),record::quote(message),record::quote(r.surface.as_str()),pose.json(),fitted.best.score,fitted.best.worst,fitted.evaluations,model.observations,model.gradients,model.empty_observations,model.posterior_iterations,model.posterior_converged,model.discrepancy,selection.considered,selection.outside);
    let mut residuals=String::from("cover,triangle,x_machine_mm,y_machine_mm,z_machine_mm,mean_distance_mm,sigma_mm,predicted_deficit_mm,measured_empty_overlap_mm\n");
    for (i, (sample, region)) in required
        .samples
        .iter()
        .zip(&fitted.best.regions)
        .enumerate()
    {
        residuals.push_str(&format!(
            "{i},{},{},{},{},{},{},{},{}\n",
            sample
                .triangle
                .map(|i| i.to_string())
                .unwrap_or_else(|| "interior".into()),
            region.point[0],
            region.point[1],
            region.point[2],
            region.mean,
            region.sigma,
            region.deficit,
            region.measured_empty
        ));
    }
    let mut history =
        String::from("evaluations,rms_predicted_deficit_mm,worst_predicted_deficit_mm\n");
    for (n, a, b) in fitted.history {
        history.push_str(&format!("{n},{a},{b}\n"));
    }
    source.source.copy_to(&output, "surface-source-")?;
    profile.export(&output.join("acquisition-profile.txt"))?;
    save(&output.join("request.txt"), &raw)?;
    save(&output.join("source.stl"), &design.raw)?;
    save(&output.join("pose-candidate.txt"), &pose_record(pose)?)?;
    save(
        &output.join("model-candidate.machine-mm.stl"),
        transformed.as_bytes(),
    )?;
    save(
        &output.join("adaptive-cycle.machine-mm.json"),
        report.as_bytes(),
    )?;
    save(&output.join("residuals.csv"), residuals.as_bytes())?;
    save(&output.join("search-history.csv"), history.as_bytes())?;
    save(
        &output.join("stock-field.machine-mm.json"),
        model.json().as_bytes(),
    )?;
    let empty_checks=model.empty_checks.iter().map(|(i,p,e,v)|format!("{{\"source\":{},\"query_machine_mm\":{},\"one_sided_residual_mm\":{e},\"latent_variance_mm2\":{v}}}",source.no_contact[*i].reference(),json(*p))).collect::<Vec<_>>().join(",");
    save(&output.join("independent-empty-checks.json"),format!("{{\"schema\":\"dmc2.adaptive-empty-checks.v1\",\"checks\":[{empty_checks}],\"trained_mean\":false}}\n").as_bytes())?;
    let (surface_fields, _) = surface::request::decode(source.source.get("request.txt")?)?;
    let cam=format!("{{\"schema\":\"dmc2.adaptive-cam-input.v1\",\"object\":{},\"setup\":{},\"analysis\":{},\"design\":{},\"frame_reference\":{},\"geometry_file\":\"model-candidate.machine-mm.stl\",\"geometry_role\":\"operation-retained-material\",\"geometry_frame\":\"LinuxCNC machine-mm\",\"geometry_import_transform\":\"identity\",\"original_geometry_file\":\"source.stl\",\"original_geometry_mm_per_unit\":{},\"original_model_mm_to_machine_mm\":{},\"stock_model_file\":\"stock-field.machine-mm.json\",\"stock_model_role\":\"noisy-estimate-with-uncertainty\",\"placement_state\":{},\"advance_to_cam_review\":{ready},\"advance_to_cutting\":false,\"accepted_physical_registration\":false,\"native_cam_job\":null,\"transform_instruction\":\"Use the positioned mesh at identity, or convert original geometry to model millimetres and apply the supplied transform once. Never apply both, and never repeat a CAD setup flip already present in its geometry.\",\"cutting_requirements\":[\"accepted setup registration\",\"actual tools and holders\",\"workholding and fixtures\",\"native CAM job and collision review\",\"reviewed cutting program\"]}}\n",record::quote(object.as_str()),record::quote(setup.as_str()),record::quote(id.as_str()),record::quote(r.design.as_str()),record::quote(&surface_fields["frame_reference"]),r.units,pose.json(),record::quote(state));
    save(&output.join("cam-input.machine-mm.json"), cam.as_bytes())?;
    let overlaps=fitted.best.empty_overlaps.iter().map(|o|format!("{{\"source\":{},\"required_volume_or_clearance_deficit_mm\":{},\"path_start_machine_mm\":{}}}",source.no_contact[o.source].json(),o.deficit,json(o.witness))).collect::<Vec<_>>().join(",");
    save(&output.join("required-volume.machine-mm.json"),format!("{{\"schema\":\"dmc2.adaptive-required-volume.v1\",\"surface_cover_samples\":{},\"interior_samples\":{},\"interior_candidates_used\":{},\"empty_space_overlaps\":[{overlaps}],\"interpretation\":\"Area-weighted surface loss plus equal-weight stochastic interior loss. Every retained finite empty capsule is compared against the unchanged required solid and requested clearance, including paths inside the required volume. Interior predictions are sampled model estimates, not physical evidence of hidden material.\"}}",covers.len(),r.interior_samples,required.interior_candidates).as_bytes())?;
    if let Some(program) = program {
        save(&output.join("adaptive-probe.ngc"), program.as_bytes())?;
    }
    let manifest=format!("{{\"schema\":\"dmc2.adaptive-stock-bundle.v1\",\"object\":{},\"setup\":{},\"analysis\":{},\"result\":{report}}}\n",record::quote(object.as_str()),record::quote(setup.as_str()),record::quote(id.as_str()));
    save(&output.join("manifest.json"), manifest.as_bytes())?;
    if matches!(outcome, Outcome::Shortage) {
        return Err(Error::PipelinePaused(crate::object_map::pipeline::Pause {
            reason: crate::object_map::pipeline::PauseReason::InsufficientMaterial,
            analysis: output,
        }));
    }
    Ok(format!("{{\"analysis_directory\":{},\"state\":{},\"message\":{},\"advance_to_cam_review\":{ready},\"advance_to_cutting\":false}}",record::quote(&output.display().to_string()),record::quote(state),record::quote(message)))
}
