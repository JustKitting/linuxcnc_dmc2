//! A proposed repeat observation uses the original acquisition request.
//! This module has no transport or machine-state access. An entry prerequisite
//! is not a transfer path, and a proposal is not an executable continuation.
use super::super::{
    mapper_schema::Phase,
    mapper_settings::{Mode, Request, Sample, Settings},
};

/// New spatial evidence, distinct from repeating an original fine contact.
/// Coordinates describe a downward column within retained acquisition bounds;
/// neither entry clearance nor the presence/height of stock is inferred.
#[derive(Clone, Copy, Debug)]
pub struct TopColumn {
    pub request: Request,
    pub entry: Entry,
    pub start: [f64; 3],
}
impl TopColumn {
    pub fn new(s: &Settings, xy: [f64; 2]) -> Result<Self, String> {
        if !matches!(s.mode, Mode::Surface | Mode::FreeSurface) {
            return Err("New top columns need a retained Surface or Automatic top map run. Select that capture; a side-trace approach cannot supply a top entry path.".into());
        }
        let request = Request::top(s, Phase::Grid, xy);
        for p in [s.origin, [xy[0], xy[1], s.origin[2]], request.target] {
            s.bounds(p)?;
        }
        Ok(Self {
            request,
            entry: Entry::OriginalClearance,
            start: s.origin,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    OriginalClearance,
    ReleasedOutlineApproach,
}
impl Entry {
    pub fn name(self) -> &'static str {
        match self {
            Self::OriginalClearance => "original-starting-clearance",
            Self::ReleasedOutlineApproach => "original-released-outline-approach",
        }
    }
    pub fn recovery(self) -> &'static str {
        match self {
            Self::OriginalClearance => {
                "Establish the same setup/probe reference and review the original starting-clearance entry before any run. No positioning move is supplied."
            }
            Self::ReleasedOutlineApproach => {
                "An outline request begins at its original released approach in the trace plane. Review a separate entry/recovery path before any run; no descent or transfer to that point was inferred."
            }
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Retouch {
    pub request: Request,
    pub entry: Entry,
    pub start: [f64; 3],
    pub original_trigger: [f64; 3],  // exact machine coordinates
    pub original_returned: [f64; 3], // reported work endpoint, not a trigger
}
impl Retouch {
    /// `sample` must come from the shared retained-cycle reader. Do not invent
    /// an approach from a corrected ball centre, stock normal or design point.
    pub fn from_sample(s: &Settings, sample: &Sample) -> Result<Self, String> {
        let original_trigger = sample.trigger.ok_or_else(|| "The source is a miss, not an original fine contact. Select a retained fine-contact cycle; no trigger was reconstructed.".to_string())?;
        let original_returned = sample.returned.ok_or_else(|| "The source cycle has no retained ready endpoint. Preserve its capture and recover through the UI before acquiring a new complete cycle.".to_string())?;
        let request = sample.request;
        let entry = match request.phase {
            Phase::Reference | Phase::Boundary | Phase::Grid | Phase::Verify
            | Phase::Rim | Phase::OutlineEnter => Entry::OriginalClearance,
            Phase::OutlineAdvance | Phase::OutlineClose => Entry::ReleasedOutlineApproach,
            Phase::OutlineBackoff | Phase::Finished => return Err("A withdrawal/finished phase is not a repeat-measurement approach. Select an original inward fine-contact cycle.".into()),
        };
        let start = match entry {
            Entry::OriginalClearance => s.origin,
            Entry::ReleasedOutlineApproach => {
                [request.approach[0], request.approach[1], request.target[2]]
            }
        };
        let approach_z = if request.phase == Phase::Rim || request.phase.is_outline() {
            request.target[2]
        } else {
            s.origin[2]
        };
        for p in [
            start,
            request.target,
            original_returned,
            [request.approach[0], request.approach[1], approach_z],
            std::array::from_fn(|i| original_trigger[i] - s.offset[i]),
        ] {
            s.bounds(p)?;
        }
        Ok(Self {
            request,
            entry,
            start,
            original_trigger,
            original_returned,
        })
    }
}
