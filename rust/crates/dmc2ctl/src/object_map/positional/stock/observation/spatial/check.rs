//! Predicted associations for fresh withheld checks; never acquisition endpoints.
use super::{grid::Budget, material};
use crate::{
    object_map::{
        positional::{
            geometry::{finite, V},
            stock::surface::local::{Checks, Local},
        },
        record::quote,
        Error,
    },
    probe_data::mapper_settings::Settings,
};
use std::collections::BTreeSet;

pub struct Prediction {
    pub source: usize,
    pub center: V,
    pub trigger: V,
}
impl Prediction {
    pub fn json(&self, a: &material::Assessment) -> String {
        let source = &a.surface.contacts[self.source];
        format!("{{\"patch_source\":{{\"capture\":{},\"sequence\":{}}},\"predicted_ball_center_machine_mm\":{:?},\"predicted_trigger_work_mm\":{:?},\"prediction_only\":true}}",quote(source.capture.as_str()),source.sequence,self.center,self.trigger)
    }
}
pub fn at(
    a: &material::Assessment,
    s: &Settings,
    locals: &[Local<'_>],
    regions: &BTreeSet<usize>,
    xy: [f64; 2],
    budget: &mut Budget,
) -> Result<Vec<Prediction>, Error> {
    budget.take(Some(locals.len()))?;
    let sr = &a.surface.request;
    let mut predictions = Vec::new();
    for l in locals {
        if l.checks != Checks::Missing
            || !regions.iter().any(|i| {
                a.regions[*i]
                    .comparisons
                    .iter()
                    .any(|c| c.source == l.station.seed)
            })
        {
            continue;
        }
        let p = l.patch;
        // A downward check needs an upward-facing offset plane. Solve its
        // intersection with the original vertical column, using the same
        // probe correction as actual measurements. This is not a new Z target.
        if p.normal[2] <= 0. {
            continue;
        }
        let mut center = [
            xy[0] + s.offset[0] + sr.probe.mount[0],
            xy[1] + s.offset[1] + sr.probe.mount[1],
            0.,
        ];
        center[2] = p.center[2]
            - (p.normal[0] * (center[0] - p.center[0]) + p.normal[1] * (center[1] - p.center[1]))
                / p.normal[2];
        let trigger = [
            xy[0],
            xy[1],
            center[2] - sr.probe.mount[2] - sr.probe.pretravel - s.offset[2],
        ];
        if !finite(center) || !finite(trigger) {
            return Err(Error::Data("A predicted check intersection cannot be represented at the retained coordinate scale. Inspect the local normal, probe correction and units; no substitute contact or endpoint was supplied.".into()));
        }
        if trigger[2] < s.floor || trigger[2] > s.origin[2] || !l.support.contains(center, p, sr)? {
            continue;
        }
        predictions.push(Prediction {
            source: l.station.seed,
            center,
            trigger,
        });
    }
    Ok(predictions)
}
