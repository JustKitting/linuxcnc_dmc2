//! Retain every source row and expose measurement needs to the acquisition planner.
use super::{
    super::{geometry::*, probe::Sample, request::Use, Error},
    fit::{self, Contour, Stop},
    request::{Closure, Request},
};
use crate::object_map::record::quote;
pub struct Report {
    pub json: String,
    pub csv: String,
    pub centers: String,
    pub refinements: String,
}
enum Need {
    Gap,
    LocalShape,
    Solve,
    Height,
    Closure,
    WallSlope,
    Check,
}
impl Need {
    fn description(&self) -> (&'static str, &'static str) {
        match self {
            Self::Gap => ("rim-gap", "Acquire intermediate rim contacts before treating this interval as supported boundary."),
            Self::LocalShape => ("local-shape-unresolved", "Resolve this region with closer contacts or a smaller fitting span; retain the observed deviation instead of classifying it as a bad measurement."),
            Self::Solve => ("local-fit-unresolved", "Inspect the retained local fit and its stopping reason; resolve fit settings or measurement support before reuse."),
            Self::Height => ("mixed-trace-heights", "Separate height levels or acquire a supported three-dimensional surface model; this XY outline cannot stand in for those surfaces."),
            Self::Closure => ("open-rim", "Continue the rim and retain an independent seam check before interpreting it as a closed outline."),
            Self::WallSlope => ("wall-slope-unmeasured", "Acquire side observations at other supported heights to estimate wall slope. A fixed-height trace alone supplies no three-dimensional stock volume."),
            Self::Check => ("independent-check-disagreement", "A withheld contact lacks an association within the requested residual. Resolve the local outline or its measurement support without silently reusing this check as fitting data."),
        }
    }
    fn json(&self, source: &str) -> String {
        let (kind, message) = self.description();
        format!("{{\"kind\":{},\"message\":{},\"source_contacts\":{},\"machine_action_authorized\":false}}",quote(kind),quote(message),source)
    }
}
pub(super) fn reference(s: &Sample) -> String {
    format!(
        "{{\"capture\":{},\"sequence\":{}}}",
        quote(s.capture.as_str()),
        s.sequence
    )
}
fn point(p: Option<V>) -> String {
    p.map(json).unwrap_or_else(|| "null".into())
}
pub(super) fn projection(p: V, a: V, b: V) -> (V, f64) {
    let d = sub(b, a);
    let l = d[0].hypot(d[1]);
    if l == 0. {
        return (a, fit::distance(p, a));
    }
    let n = [d[0] / l, d[1] / l, 0.];
    let t = (dot(sub(p, a), n) / l).clamp(0., 1.);
    let q = add(a, scale(d, t));
    (q, fit::distance(p, q))
}
pub fn build(samples: &[Sample], contour: &Contour, r: &Request) -> Result<Report, Error> {
    let mut needs = Vec::new();
    let mut station_json = Vec::new();
    for station in &contour.stations {
        let s = &samples[station.sample];
        let refs = station
            .neighbours
            .iter()
            .map(|i| reference(&samples[*i]))
            .collect::<Vec<_>>()
            .join(",");
        let max = station
            .residuals
            .iter()
            .map(|x| x.abs())
            .fold(0_f64, f64::max);
        if max > r.max_residual {
            needs.push(Need::LocalShape.json(&format!("[{refs}]")));
        }
        if station.stop != Stop::Converged {
            needs.push(Need::Solve.json(&format!("[{}]", reference(s))));
        }
        let rows = station
            .neighbours
            .iter()
            .zip(&station.residuals)
            .zip(&station.weights)
            .map(|((i, d), w)| {
                format!(
                    "{{\"source\":{},\"perpendicular_residual_mm\":{},\"huber_weight\":{}}}",
                    reference(&samples[*i]),
                    d,
                    w
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        station_json.push(format!("{{\"source\":{},\"fitted_center_machine_mm\":{},\"outward_xy_normal\":{},\"surface_estimate_machine_mm\":{},\"stop\":{},\"iterations\":{},\"objective_start_mm2\":{},\"objective_end_mm2\":{},\"neighbourhood\":[{}]}}",reference(s),json(station.center),json(station.normal),point(station.surface),quote(station.stop.name()),station.iterations,station.objective_start,station.objective_end,rows));
    }
    let mut edges = Vec::new();
    let count = contour.stations.len();
    for i in 0..count - usize::from(r.closure == Closure::Open) {
        let j = (i + 1) % count;
        let a = &contour.stations[i];
        let b = &contour.stations[j];
        let gap = fit::distance(samples[a.sample].center, samples[b.sample].center);
        let supported = gap <= r.max_gap;
        if !supported {
            needs.push(Need::Gap.json(&format!(
                "[{},{}]",
                reference(&samples[a.sample]),
                reference(&samples[b.sample])
            )));
        }
        edges.push((i, j, supported, gap));
    }
    let zmin = contour
        .stations
        .iter()
        .map(|p| p.center[2])
        .fold(f64::INFINITY, f64::min);
    let zmax = contour
        .stations
        .iter()
        .map(|p| p.center[2])
        .fold(f64::NEG_INFINITY, f64::max);
    if zmax - zmin > r.max_z_span {
        needs.push(Need::Height.json("[]"));
    }
    if r.closure == Closure::Open || !samples.iter().any(|s| s.usage == Use::Check) {
        needs.push(Need::Closure.json("[]"));
    }
    needs.push(Need::WallSlope.json("[]"));
    let mut residuals=String::from("capture,sequence,use,capture_state,trigger_machine_x_mm,trigger_machine_y_mm,trigger_machine_z_mm,center_machine_x_mm,center_machine_y_mm,center_machine_z_mm,commanded_feed_mm_min,approach_x,approach_y,approach_z,nearest_supported_center_x_mm,nearest_supported_center_y_mm,nearest_supported_center_z_mm,horizontal_center_residual_mm\n");
    let mut centers = String::new();
    let mut checks = Vec::new();
    for s in samples {
        // No nearest-line check spans a gap beyond the requested support bound.
        let nearest = edges
            .iter()
            .filter(|e| e.2)
            .map(|(i, j, _, _)| {
                projection(
                    s.center,
                    contour.stations[*i].center,
                    contour.stations[*j].center,
                )
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let (position, distance) = nearest
            .map(|(p, d)| (csv(p), d.to_string()))
            .unwrap_or_else(|| (",,".into(), String::new()));
        residuals.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            s.capture.as_str(),
            s.sequence,
            s.usage.name(),
            s.state.name(),
            csv(s.trigger),
            csv(s.center),
            s.feed,
            csv(s.approach),
            position,
            distance
        ));
        centers.push_str(&format!("{}\n", csv(s.center).replace(',', " ")));
        if s.usage == Use::Check {
            if !nearest.is_some_and(|x| x.1 <= r.max_residual) {
                needs.push(Need::Check.json(&format!("[{}]", reference(s))));
            }
            checks.push(format!("{{\"source\":{},\"horizontal_center_residual_mm\":{},\"within_requested_residual\":{}}}",reference(s),nearest.map(|x|x.1.to_string()).unwrap_or_else(||"null".into()),nearest.is_some_and(|x|x.1<=r.max_residual)));
        }
    }
    let edge_json=edges.iter().map(|(a,b,s,g)|format!("{{\"from_station\":{a},\"to_station\":{b},\"measured_xy_gap_mm\":{g},\"within_requested_gap\":{s},\"between_contacts\":\"interpolation-assumption\"}}")).collect::<Vec<_>>().join(",");
    let refinements=format!("{{\"schema\":\"dmc2.stock-measurement-needs.v1\",\"needs\":[{}],\"execution\":\"unplanned-observation-requirements\",\"machine_commands_issued\":false}}\n",needs.join(","));
    let json=format!("{{\"schema\":\"dmc2.stock-outline.v1\",\"state\":\"unreviewed-stock-outline\",\"frame\":\"LinuxCNC machine-mm\",\"calibration_state\":{},\"declared_closure\":{},\"surface_model\":{},\"trace_center_z_range_mm\":[{},{}],\"stations\":[{}],\"edges\":[{}],\"independent_checks\":[{}],\"measurement_needs\":{},\"solid_stock\":null,\"unmeasured_volume\":\"unknown\",\"self_intersections_checked\":false,\"cam_ready\":false,\"correction_formula\":\"center = original_trigger + trigger_to_ball - pretravel * approach; vertical-side surface estimate = fitted_center - radius * fitted_outward_XY_normal\",\"interpretation\":\"Local iterative Huber regression of the ordered ball-centre contour, without an STL, rectangle or fixed side count. Neighbourhood residuals retain every fitting row. Check/observe rows never drive the fit. Interpolated segments, vertical sides and declared closure are modeling assumptions, not measured stock volume. The surface estimate applies only when vertical-sides was explicitly selected. Sharp corners, wall slope and inaccessible concavities need further evidence. No placement, motion or machine offset is produced.\"}}\n",quote(r.probe.calibration.name()),quote(r.closure.name()),quote(r.surface.name()),zmin,zmax,station_json.join(","),edge_json,checks.join(","),refinements);
    if [&json, &residuals].iter().any(|s| {
        s.contains("NaN")
            || s.contains(":inf")
            || s.contains(":-inf")
            || s.contains(",inf")
            || s.contains(",-inf")
    }) {
        return Err(Error::Data(
            "Stock outline report arithmetic overflowed; inspect units and contact coordinates."
                .into(),
        ));
    }
    Ok(Report {
        json,
        csv: residuals,
        centers,
        refinements,
    })
}
