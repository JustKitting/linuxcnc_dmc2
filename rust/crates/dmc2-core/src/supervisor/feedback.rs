use crate::PULSES_PER_MM;
use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};

use super::FaultCode;

const STEPGEN_PHASE_EPSILON_PULSES: f64 = 0.000_001;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StepgenFeedbackContext {
    Jog,
    LimitRecovery,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StepgenFeedbackError {
    JogUnavailable,
    JogIncoherent,
    LimitRecoveryUnavailable,
    LimitRecoveryIncoherent,
}

impl StepgenFeedbackContext {
    const fn unavailable(self) -> StepgenFeedbackError {
        match self {
            Self::Jog => StepgenFeedbackError::JogUnavailable,
            Self::LimitRecovery => StepgenFeedbackError::LimitRecoveryUnavailable,
        }
    }

    const fn incoherent(self) -> StepgenFeedbackError {
        match self {
            Self::Jog => StepgenFeedbackError::JogIncoherent,
            Self::LimitRecovery => StepgenFeedbackError::LimitRecoveryIncoherent,
        }
    }
}

impl StepgenFeedbackError {
    pub(super) const fn fault_code(self) -> FaultCode {
        match self {
            Self::JogUnavailable => FaultCode::JogFeedbackUnavailable,
            Self::JogIncoherent => FaultCode::JogFeedbackIncoherent,
            Self::LimitRecoveryUnavailable => FaultCode::BounceFeedbackUnavailable,
            Self::LimitRecoveryIncoherent => FaultCode::BounceFeedbackIncoherent,
        }
    }
}

impl RecoveryClassified for StepgenFeedbackError {
    fn recovery_class(&self) -> RecoveryClass {
        self.fault_code().recovery_class()
    }
}

/// Converts HostMot2's position feedback to generated pulses while proving it
/// is coherent with HostMot2's arithmetic-shifted signed count.
pub(super) fn stepgen_position_pulses(
    generated_count: i32,
    position_feedback_mm: f64,
    context: StepgenFeedbackContext,
) -> Result<f64, StepgenFeedbackError> {
    let position_pulses = position_feedback_mm * PULSES_PER_MM as f64;
    if !position_pulses.is_finite() {
        return Err(context.unavailable());
    }
    let fractional_phase = position_pulses - generated_count as f64;
    if !(-STEPGEN_PHASE_EPSILON_PULSES..1.0 + STEPGEN_PHASE_EPSILON_PULSES)
        .contains(&fractional_phase)
    {
        return Err(context.incoherent());
    }
    Ok(position_pulses)
}
