use crate::PULSES_PER_MM;

const STEPGEN_PHASE_EPSILON_PULSES: f64 = 0.000_001;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StepgenFeedbackError {
    Unavailable,
    Incoherent,
}

/// Converts HostMot2's position feedback to generated pulses while proving it
/// is coherent with HostMot2's arithmetic-shifted signed count.
pub(super) fn stepgen_position_pulses(
    generated_count: i32,
    position_feedback_mm: f64,
) -> Result<f64, StepgenFeedbackError> {
    let position_pulses = position_feedback_mm * PULSES_PER_MM as f64;
    if !position_pulses.is_finite() {
        return Err(StepgenFeedbackError::Unavailable);
    }
    let fractional_phase = position_pulses - generated_count as f64;
    if !(-STEPGEN_PHASE_EPSILON_PULSES..1.0 + STEPGEN_PHASE_EPSILON_PULSES)
        .contains(&fractional_phase)
    {
        return Err(StepgenFeedbackError::Incoherent);
    }
    Ok(position_pulses)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_exact_negative_live_hostmot2_capture() {
        let value = stepgen_position_pulses(-11, -0.010_003_16).unwrap();
        assert!((value - -10.00316).abs() < 1e-12);
    }

    #[test]
    fn rejects_non_finite_and_count_incoherent_feedback() {
        assert_eq!(
            stepgen_position_pulses(0, f64::NAN),
            Err(StepgenFeedbackError::Unavailable)
        );
        assert_eq!(
            stepgen_position_pulses(-10, -0.010_003_16),
            Err(StepgenFeedbackError::Incoherent)
        );
    }
}
