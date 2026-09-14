//! Nominal-ball estimates kept separate from original trigger coordinates.
use super::super::{
    geometry::{dot, Rectangle},
    ledger::{number, Fields},
};
use super::model::{xyz, Mode, Settings};

enum Source {
    TopEnvelope,
    OpposingSides,
}
pub(super) struct Estimate {
    size: [f64; 2],
    source: Source,
}

impl Estimate {
    pub fn read(records: &[Fields], s: &Settings, rect: Rectangle) -> Result<Option<Self>, String> {
        let (size, source) = if s.mode == Mode::Surface {
            (
                [0, 1].map(|i| rect.max[i] - rect.min[i] - 2.0 * s.radius),
                Source::TopEnvelope,
            )
        } else {
            let mut supports: [Vec<f64>; 4] = std::array::from_fn(|_| Vec::new());
            for r in records
                .iter()
                .skip(1)
                .filter(|r| r["kind"] == "touch" && r["stage"] == "1" && r["phase"] == "4")
            {
                let edge = number(r, "edge")? as usize;
                if edge >= supports.len() {
                    return Err("Rim dimension has an invalid face identifier.".into());
                }
                let p = xyz(r, "machine_", "_exact")?;
                let sign = if edge % 2 == 0 { -1.0 } else { 1.0 };
                let normal = [rect.u, rect.v][edge / 2].map(|v| v * sign);
                // Outward normal dot ball centre minus radius gives nominal
                // surface support. Opposing supports sum to the width; a fixed
                // mounted XY offset cancels in that sum.
                supports[edge].push(dot(normal, [p[0], p[1]]) - s.radius);
            }
            if supports.iter().any(Vec::is_empty) {
                return Ok(None);
            }
            let median = |mut values: Vec<f64>| {
                values.sort_by(f64::total_cmp);
                (values[(values.len() - 1) / 2] + values[values.len() / 2]) / 2.0
            };
            let support = supports.map(median);
            (
                [support[0] + support[1], support[2] + support[3]],
                Source::OpposingSides,
            )
        };
        if size.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Ok(None);
        }
        Ok(Some(Self { size, source }))
    }
    pub fn json(&self) -> String {
        let (source,formula,assumptions) = match self.source {
            Source::TopEnvelope => ("top_contact_envelope","fitted contact span - 2 * nominal ball radius","Rough estimate: approximately vertical sides; search floor at least a ball radius below local top. Shallow edge contacts can have less than the full ball-radius extension."),
            Source::OpposingSides => ("opposing_fine_side_contacts","median(outward normal dot original machine XY - nominal radius) for one face + the opposing face median","Face normals from the fitted footprint; nominal ball radius; trigger pretravel is uncalibrated. These dimensions describe the sampled side height."),
        };
        format!("{{\"size_along_fitted_uv_mm\":{:?},\"source\":\"{source}\",\"formula\":\"{formula}\",\"assumptions\":\"{assumptions}\",\"calibrated_accuracy_mm\":null}}",self.size)
    }
}
