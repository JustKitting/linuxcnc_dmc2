//! One material decision for CLI failure, inspection and downstream CAD metadata.
use super::{query::State, Assessment};
use crate::object_map::{
    pipeline::{Pause, PauseReason},
    record::quote,
    Error,
};
use std::path::Path;

pub struct Decision {
    pub reason: PauseReason,
    blockers: Vec<(PauseReason, String)>,
}
impl Decision {
    pub fn from(a: &Assessment) -> Self {
        let mut blockers = Vec::new();
        for (region, result) in a.regions.iter().enumerate() {
            let reason = match result.state {
                State::Shortage | State::EmptyOverlap => PauseReason::InsufficientMaterial,
                State::CheckConflict | State::NoContactConflict => {
                    PauseReason::ConflictingMeasurements
                }
                State::Unsupported
                | State::Unchecked
                | State::EmptyBoundary
                | State::BoundaryBand => PauseReason::UnresolvedCoverage,
                State::LocallyInward => continue,
            };
            blockers.push((reason, format!("{{\"file\":\"material-check.machine-mm.json\",\"region\":{region},\"source_triangle\":{},\"machine_center_mm\":{},\"measurement_state\":{}}}",a.covers[region].triangle,super::super::super::geometry::json(a.candidate.pose.point(a.covers[region].center)),quote(result.state.description().0))));
        }
        if let Some(volume) = &a.volume {
            blockers.extend(volume.pipeline_blockers(&a.surface.contacts));
        }
        // This assessment models local support, not complete stock occupancy.
        // A clear local comparison cannot release an unresolved stock volume.
        blockers.push((PauseReason::UnresolvedCoverage, "{\"file\":\"measurement-needs.json\",\"kind\":\"closed-material-coverage-unresolved\"}".into()));
        let reason = if blockers
            .iter()
            .any(|(r, _)| *r == PauseReason::InsufficientMaterial)
        {
            PauseReason::InsufficientMaterial
        } else if blockers
            .iter()
            .any(|(r, _)| *r == PauseReason::ConflictingMeasurements)
        {
            PauseReason::ConflictingMeasurements
        } else {
            PauseReason::UnresolvedCoverage
        };
        Self { reason, blockers }
    }
    pub fn json(&self) -> String {
        let (reason, message, recovery) = self.reason.description();
        let blockers = self
            .blockers
            .iter()
            .map(|(r, evidence)| {
                format!(
                    "{{\"reason\":{},\"evidence\":{evidence}}}",
                    quote(r.description().0)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{{\"schema\":\"dmc2.material-pipeline-state.v1\",\"state\":\"paused\",\"reason\":{},\"message\":{},\"recovery\":{},\"blockers\":[{blockers}],\"advance_to_cutting\":false,\"cam_ready\":false,\"machine_action_authorized\":false}}\n",quote(reason),quote(message),quote(recovery))
    }
    pub fn result(&self, output: &Path) -> Result<String, Error> {
        match self.reason {
            PauseReason::InsufficientMaterial | PauseReason::ConflictingMeasurements => Err(Error::PipelinePaused(Pause { reason: self.reason, analysis: output.to_owned() })),
            PauseReason::UnresolvedCoverage => Ok(format!("{{\"analysis_directory\":{},\"state\":{},\"message\":{},\"pipeline\":{},\"cam_ready\":false}}",quote(&output.display().to_string()),quote(self.reason.description().0),quote(self.reason.description().1),self.json())),
        }
    }
}
