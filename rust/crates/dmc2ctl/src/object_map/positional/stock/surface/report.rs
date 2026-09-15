use super::super::super::{probe::Sample, request::Use, Error};
use super::super::{fit::Stop, report::Report};
use super::support::Support;
use super::{geometry::*, request::Request, Station};
use crate::object_map::record::quote;
fn reference(s: &Sample) -> String {
    format!(
        "{{\"capture\":{},\"sequence\":{}}}",
        quote(s.capture.as_str()),
        s.sequence
    )
}
pub fn build(samples: &[Sample], stations: &[Station], r: &Request) -> Result<Report, Error> {
    let mut patches = Vec::new();
    let mut needs = Vec::new();
    let mut supports = Vec::new();
    let need = |kind: &str, message: &str, refs: &str| {
        format!("{{\"kind\":{},\"message\":{},\"source_contacts\":{},\"machine_action_authorized\":false}}",quote(kind),quote(message),refs)
    };
    for station in stations {
        let refs = format!(
            "[{}]",
            station
                .neighbours
                .iter()
                .map(|i| reference(&samples[*i]))
                .collect::<Vec<_>>()
                .join(",")
        );
        let source = reference(&samples[station.seed]);
        match &station.result {
            Err(reason) => {
                let (kind, message) = reason.description();
                needs.push(need(kind, message, &format!("[{source}]")));
                patches.push(format!("{{\"source\":{source},\"state\":\"unresolved\",\"reason\":{},\"message\":{},\"neighbours\":{refs}}}",quote(kind),quote(message)));
            }
            Ok(p) => {
                let max = p.residuals.iter().map(|x| x.abs()).fold(0_f64, f64::max);
                let support = Support::new(samples, station, p, r)?;
                let eligible = p.stop == Stop::Converged
                    && max <= r.max_residual
                    && support.conflicts.is_empty();
                if p.stop != Stop::Converged {
                    needs.push(need("surface-fit-unresolved","The local fit did not reach its requested step tolerance. Inspect its stopping reason and solve settings before reusing this patch.",&refs));
                }
                if max > r.max_residual {
                    needs.push(need("surface-shape-unresolved","Local contacts depart from the fitted plane beyond the requested residual. Refine the region or fitting scale; coherent shape and every original row remain retained.",&refs));
                }
                for conflict in &support.conflicts {
                    needs.push(support.conflict_json(conflict, samples, station, p));
                }
                let rows=station.neighbours.iter().zip(&p.residuals).zip(&p.weights).map(|((i,d),w)|format!("{{\"source\":{},\"perpendicular_residual_mm\":{d},\"huber_weight\":{w}}}",reference(&samples[*i]))).collect::<Vec<_>>().join(",");
                patches.push(format!("{{\"source\":{source},\"state\":\"estimated-local-plane\",\"fitted_center_machine_mm\":{},\"surface_machine_mm\":{},\"outward_normal\":{},\"weighted_covariance_eigenvalues_mm2\":{},\"stop\":{},\"iterations\":{},\"objective_start_mm2\":{},\"objective_end_mm2\":{},\"eligible_for_local_checks\":{eligible},\"support\":{},\"neighbourhood\":[{rows}]}}",json(p.center),json(p.surface),json(p.normal),json(p.variance),quote(p.stop.name()),p.iterations,p.objective_start,p.objective_end,support.json(p,r)));
                supports.push((station, p, support, eligible));
            }
        }
    }
    let mut csv_rows=String::from("capture,sequence,use,capture_state,trigger_machine_x_mm,trigger_machine_y_mm,trigger_machine_z_mm,center_machine_x_mm,center_machine_y_mm,center_machine_z_mm,commanded_feed_mm_min,approach_x,approach_y,approach_z,associated_patch_capture,associated_patch_sequence,perpendicular_center_residual_mm\n");
    let mut centers = String::new();
    let mut checks = Vec::new();
    for s in samples {
        let mut candidates = Vec::new();
        for (station, p, support, eligible) in &supports {
            if *eligible && dot(p.normal, s.approach) < 0. && support.contains(s.center, p, r)? {
                candidates.push((*station, dot(sub(s.center, p.center), p.normal)));
            }
        }
        let nearest = candidates
            .into_iter()
            .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()));
        let association = nearest
            .map(|(p, d)| {
                format!(
                    "{},{},{d}",
                    samples[p.seed].capture.as_str(),
                    samples[p.seed].sequence
                )
            })
            .unwrap_or_else(|| ",,".into());
        csv_rows.push_str(&format!(
            "{},{},{},{},{},{},{},{},{association}\n",
            s.capture.as_str(),
            s.sequence,
            s.usage.name(),
            s.state.name(),
            csv(s.trigger),
            csv(s.center),
            s.feed,
            csv(s.approach)
        ));
        centers.push_str(&format!("{}\n", csv(s.center).replace(',', " ")));
        if s.usage == Use::Check {
            let within = nearest.is_some_and(|(_, d)| d.abs() <= r.max_residual);
            if !within {
                needs.push(need("independent-surface-check-disagreement","This withheld contact lacks a supported local plane within the requested residual. Resolve the support or measured surface; do not silently turn it into fitting data.",&format!("[{}]",reference(s))));
            }
            checks.push(format!("{{\"source\":{},\"associated_patch\":{},\"perpendicular_center_residual_mm\":{},\"within_requested_residual\":{within}}}",reference(s),nearest.map(|(p,_)|reference(&samples[p.seed])).unwrap_or_else(||"null".into()),nearest.map(|(_,d)|d.to_string()).unwrap_or_else(||"null".into())));
        }
    }
    needs.push(need("stock-volume-coverage-unresolved","Local surface patches do not establish a closed stock volume. Retain top, side and supporting-base coverage and resolve open or contradictory regions before material containment.","[]"));
    if checks.is_empty() {
        needs.push(need("independent-surface-check-missing","No contacts were withheld to check these surfaces. Retain independent observations in the same setup reference before accepting the estimate.","[]"));
    }
    let refinements=format!("{{\"schema\":\"dmc2.stock-measurement-needs.v1\",\"needs\":[{}],\"execution\":\"unplanned-observation-requirements\",\"machine_commands_issued\":false}}\n",needs.join(","));
    let (schema, no_contact) = match r.no_contact {
        super::request::NoContactModel::Legacy => ("dmc2.stock-surface.v1", String::new()),
        super::request::NoContactModel::ErodedProbeSweep { allowance } => {
            let (schema, selection) = match &r.no_contact_sources {
                super::request::NoContactSources::ContributingContacts => ("dmc2.stock-surface.v2", String::new()),
                super::request::NoContactSources::Explicit(entries) => ("dmc2.stock-surface.v3", format!(",\"capture_decisions\":[{}],\"source_policy\":\"Explicit no-contact captures are independent of fine-contact fit/check selections. Reasons are request annotations; review the shared physical frame and calibration. Excluded sources do not establish material presence.\"",entries.iter().map(|e| e.json()).collect::<Vec<_>>().join(","))),
            };
            let misses = stations
                .first()
                .map(|s| s.no_contact.as_ref())
                .unwrap_or(&[]);
            let observations = misses
                .iter()
                .map(|m| m.json())
                .collect::<Vec<_>>()
                .join(",");
            (schema, format!(",\"no_contact_model\":{{\"kind\":\"eroded-probe-sweep\",\"ball_radius_mm\":{},\"pretravel_mm\":{},\"additional_allowance_mm\":{allowance},\"sweeps\":[{observations}]{selection},\"interpretation\":\"Use every original coarse miss in the selected captures with its matching reported endpoint and retained settings. Centre path = reported machine path + mounting vector. Eroded radius = ball radius - declared pretravel - additional allowance. This assumes their sum bounds undetected contact and path-position error; it is not certified empty space. Unmeasured volume remains unknown.\"}}",r.probe.radius,r.probe.pretravel))
        }
    };
    let json=format!("{{\"schema\":\"{schema}\",\"state\":\"unreviewed-stock-surface\",\"frame\":\"LinuxCNC machine-mm\",\"calibration_state\":{},\"patches\":[{}],\"independent_checks\":[{}],\"measurement_needs\":{},\"solid_stock\":null,\"unmeasured_volume\":\"unknown\",\"cam_ready\":false,\"correction_formula\":\"center = original_trigger + trigger_to_ball - pretravel * approach; local surface = fitted_center - ball_radius * fitted_outward_normal\",\"interpretation\":\"Local iterative Huber orthogonal plane fits of retained 3D ball-centre observations. Neighbourhood radius and approach grouping are explicit fit assumptions. Approach determines normal sign, not wall slope. All fit/check/observe rows remain retained; independent checks never drive fitting. Local planes approximate the offset surface and do not recover inaccessible concavities or prove material coverage. No nominal CAD, box dimensions, work offsets or machine commands are supplied by this analysis.\"{no_contact}}}\n",quote(r.probe.calibration.name()),patches.join(","),checks.join(","),refinements);
    if [&json, &csv_rows].iter().any(|s| {
        s.contains("NaN")
            || s.contains(":inf")
            || s.contains(":-inf")
            || s.contains(",inf")
            || s.contains(",-inf")
    }) {
        return Err(Error::Data("Surface report arithmetic overflowed; inspect coordinate units and support scales before reusing this analysis.".into()));
    }
    Ok(Report {
        json,
        csv: csv_rows,
        centers,
        refinements,
    })
}
