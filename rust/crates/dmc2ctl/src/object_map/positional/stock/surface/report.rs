use super::super::super::{probe::Sample, request::Use, Error};
use super::super::{fit::Stop, report::Report};
use super::{geometry::*, request::Request, Patch, Station};
use crate::object_map::record::quote;
type P = [f64; 2];
fn reference(s: &Sample) -> String {
    format!(
        "{{\"capture\":{},\"sequence\":{}}}",
        quote(s.capture.as_str()),
        s.sequence
    )
}
fn cross2(a: P, b: P, c: P) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
struct Support {
    u: V,
    v: V,
    hull: Vec<P>,
    points: Vec<P>,
}
impl Support {
    fn new(samples: &[Sample], station: &Station, patch: &Patch, r: &Request) -> Self {
        // This basis parameterizes the measured plane; it does not align the
        // stock to a machine axis or supply a missing surface normal.
        let axis = (0..3)
            .min_by(|a, b| patch.normal[*a].abs().total_cmp(&patch.normal[*b].abs()))
            .unwrap();
        let mut direction = [0.; 3];
        direction[axis] = 1.;
        let u = cross(patch.normal, direction);
        let u = u.map(|x| x / norm(u));
        let v = cross(patch.normal, u);
        let points = station
            .neighbours
            .iter()
            .map(|i| {
                let d = sub(samples[*i].center, patch.center).map(|x| x / r.neighborhood);
                [dot(d, u), dot(d, v)]
            })
            .collect::<Vec<_>>();
        let mut sorted = points.clone();
        sorted.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
        sorted.dedup();
        let mut hull = Vec::new();
        for seq in [sorted.clone(), sorted.into_iter().rev().collect()] {
            let mut half: Vec<P> = Vec::new();
            for p in seq {
                while half.len() >= 2 && cross2(half[half.len() - 2], half[half.len() - 1], p) <= 0.
                {
                    half.pop();
                }
                half.push(p);
            }
            half.pop();
            hull.extend(half);
        }
        Self { u, v, hull, points }
    }
    fn contains(&self, p: V, patch: &Patch, r: &Request) -> bool {
        let d = sub(p, patch.center).map(|x| x / r.neighborhood);
        let q = [dot(d, self.u), dot(d, self.v)];
        if self.hull.len() < 3 || !q.iter().all(|x| x.is_finite()) {
            return false;
        }
        for (a, b) in self
            .hull
            .iter()
            .zip(self.hull.iter().cycle().skip(1))
            .take(self.hull.len())
        {
            let first = (b[0] - a[0]) * (q[1] - a[1]);
            let second = (b[1] - a[1]) * (q[0] - a[0]);
            let roundoff = f64::EPSILON * (self.hull.len() as f64) * (first.abs() + second.abs());
            if first - second < -roundoff {
                return false;
            }
        }
        self.points
            .iter()
            .any(|p| (p[0] - q[0]).hypot(p[1] - q[1]) * r.neighborhood <= r.support_gap)
    }
    fn json(&self, patch: &Patch, r: &Request) -> String {
        let vertices = self
            .hull
            .iter()
            .map(|p| {
                json(add(
                    patch.surface,
                    add(
                        scale(self.u, p[0] * r.neighborhood),
                        scale(self.v, p[1] * r.neighborhood),
                    ),
                ))
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{{\"projected_neighbour_hull_machine_mm\":[{vertices}],\"max_nearest_sample_distance_mm\":{},\"interpretation\":\"Local planar interpolation assumption inside the projected neighbour hull and within the selected distance of a neighbour; this is not observed material coverage or a closed solid.\"}}",r.support_gap)
    }
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
                let eligible = p.stop == Stop::Converged && max <= r.max_residual;
                if p.stop != Stop::Converged {
                    needs.push(need("surface-fit-unresolved","The local fit did not reach its requested step tolerance. Inspect its stopping reason and solve settings before reusing this patch.",&refs));
                }
                if max > r.max_residual {
                    needs.push(need("surface-shape-unresolved","Local contacts depart from the fitted plane beyond the requested residual. Refine the region or fitting scale; coherent shape and every original row remain retained.",&refs));
                }
                let support = Support::new(samples, station, p, r);
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
        let nearest = supports
            .iter()
            .filter(|(_, p, support, eligible)| {
                *eligible && dot(p.normal, s.approach) < 0. && support.contains(s.center, p, r)
            })
            .map(|(station, p, _, _)| (station, dot(sub(s.center, p.center), p.normal)))
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
    let json=format!("{{\"schema\":\"dmc2.stock-surface.v1\",\"state\":\"unreviewed-stock-surface\",\"frame\":\"LinuxCNC machine-mm\",\"calibration_state\":{},\"patches\":[{}],\"independent_checks\":[{}],\"measurement_needs\":{},\"solid_stock\":null,\"unmeasured_volume\":\"unknown\",\"cam_ready\":false,\"correction_formula\":\"center = original_trigger + trigger_to_ball - pretravel * approach; local surface = fitted_center - ball_radius * fitted_outward_normal\",\"interpretation\":\"Local iterative Huber orthogonal plane fits of retained 3D ball-centre observations. Neighbourhood radius and approach grouping are explicit fit assumptions. Approach determines normal sign, not wall slope. All fit/check/observe rows remain retained; independent checks never drive fitting. Local planes approximate the offset surface and do not recover inaccessible concavities or prove material coverage. No nominal CAD, box dimensions, work offsets or machine commands are supplied by this analysis.\"}}\n",quote(r.probe.calibration.name()),patches.join(","),checks.join(","),refinements);
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
