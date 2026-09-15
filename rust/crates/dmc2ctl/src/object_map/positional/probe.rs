//! One calibration and original-fine-contact path for model and stock analysis.
use super::{
    super::{
        capture::Capture,
        model::{CaptureState, Id, Stage},
        store::CaptureSnapshot,
        Error,
    },
    geometry::*,
    request::{scalar, vector, Selection, Use},
};
use std::collections::BTreeMap;

pub fn keys(extra: &[&'static str]) -> Vec<&'static str> {
    [
        "calibration_state",
        "calibration_reference",
        "frame_reference",
        "ball_radius_mm",
        "trigger_to_ball_mm",
        "pretravel_mm",
    ]
    .into_iter()
    .chain(extra.iter().copied())
    .collect()
}

#[derive(Clone, Copy)]
pub enum Calibration {
    Nominal,
    Calibrated,
    Synthetic,
}
impl Calibration {
    pub fn name(self) -> &'static str {
        match self {
            Self::Nominal => "nominal",
            Self::Calibrated => "calibrated",
            Self::Synthetic => "synthetic",
        }
    }
}
pub struct Probe {
    pub calibration: Calibration,
    pub radius: f64,
    pub mount: V,
    pub pretravel: f64,
}
pub struct Sample {
    pub capture: Id,
    pub sequence: usize,
    pub usage: Use,
    pub state: CaptureState,
    pub trigger: V,
    pub center: V,
    pub approach: V,
    pub feed: f64,
}
impl Probe {
    pub fn read(f: &BTreeMap<String, String>) -> Result<Self, Error> {
        let radius = scalar(&f["ball_radius_mm"], "ball_radius_mm")?;
        if radius <= 0. {
            return Err(Error::Input("ball_radius_mm must be positive.".into()));
        }
        let pretravel = scalar(&f["pretravel_mm"], "pretravel_mm")?;
        if pretravel < 0. {
            return Err(Error::Input("pretravel_mm is a nonnegative distance along the recorded approach; it is subtracted from trigger position.".into()));
        }
        let calibration = match f["calibration_state"].as_str() {
            "nominal" => Calibration::Nominal,
            "calibrated" => Calibration::Calibrated,
            "synthetic" => Calibration::Synthetic,
            _ => {
                return Err(Error::Input(
                    "calibration_state must be nominal, calibrated or synthetic.".into(),
                ))
            }
        };
        for key in ["calibration_reference", "frame_reference"] {
            if f[key] == "REQUIRED" {
                return Err(Error::Input(format!("{key} must identify calibration evidence and the shared mounting/homing reference for these captures.")));
            }
        }
        Ok(Self {
            calibration,
            radius,
            mount: vector(&f["trigger_to_ball_mm"], "trigger_to_ball_mm")?,
            pretravel,
        })
    }
    pub fn measure(&self, id: &Id, c: &Capture, selected: &Selection) -> Result<Sample, Error> {
        if c.state == CaptureState::Quarantined {
            return Err(Error::Data(format!("Capture {} is quarantined. Preserve it for inspection; recapture required geometry before fitting.",id.as_str())));
        }
        let p = c.contacts.iter().find(|p| p.sequence == selected.sequence && p.stage == Stage::Fine)
            .ok_or_else(|| Error::Data(format!("{}:{} is not an original fine contact; endpoints, coarse touches, releases and misses cannot substitute.",id.as_str(),selected.sequence)))?;
        let center = sub(
            add(p.trigger_mm, self.mount),
            scale(p.direction, self.pretravel),
        );
        if !finite(center) {
            return Err(Error::Data(
                "Probe correction overflowed; check the request calibration.".into(),
            ));
        }
        Ok(Sample {
            capture: id.clone(),
            sequence: p.sequence,
            usage: selected.usage,
            state: c.state,
            trigger: p.trigger_mm,
            center,
            approach: p.direction,
            feed: p.commanded_feed_mm_min,
        })
    }
    pub fn samples(
        &self,
        captures: &[CaptureSnapshot],
        selections: &[Selection],
    ) -> Result<Vec<Sample>, Error> {
        selections
            .iter()
            .map(|s| {
                let c = captures.iter().find(|c| c.id == s.capture).ok_or_else(|| {
                    Error::Data(format!(
                        "Capture {} is absent from this setup.",
                        s.capture.as_str()
                    ))
                })?;
                self.measure(&c.id, &c.capture, s)
            })
            .collect()
    }
}
