//! Offline job decisions; these never own machine faults or operator controls.
use std::{fmt, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseReason {
    InsufficientMaterial,
    ConflictingMeasurements,
    UnresolvedCoverage,
}
impl PauseReason {
    pub fn description(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::InsufficientMaterial => (
                "insufficient-material",
                "Insufficient material for this placement and its requested clearance. The mapping-to-cutting pipeline is paused.",
                "Inspect the retained shortage locations. Select sufficient stock or explicitly revise the placement, then run a new material check. Required geometry and clearance have not been changed.",
            ),
            Self::ConflictingMeasurements => (
                "conflicting-material-measurements",
                "The material evidence contains conflicting measurements. The mapping-to-cutting pipeline is paused.",
                "Resolve the named source/reference disagreement and run a new material check with the corrected evidence. Both original observations remain retained.",
            ),
            Self::UnresolvedCoverage => (
                "material-coverage-unresolved",
                "Material coverage remains unresolved. The mapping-to-cutting pipeline is paused.",
                "Use the retained measurement needs to establish the missing material coverage, then run a new material check. Unknown material is not treated as sufficient.",
            ),
        }
    }
}

#[derive(Debug)]
pub struct Pause {
    pub reason: PauseReason,
    pub analysis: PathBuf,
}
impl fmt::Display for Pause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (_, message, recovery) = self.reason.description();
        write!(f, "{message} {recovery} Evidence: {}. Open Inspect analysis in Object Mapper to read the pipeline decision and shortage records. Clear Fault and Pendant Mode remain independent of this offline job.", self.analysis.display())
    }
}
