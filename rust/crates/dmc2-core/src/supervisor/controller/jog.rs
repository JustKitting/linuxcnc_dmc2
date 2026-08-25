use super::super::feedback::{stepgen_position_pulses, StepgenFeedbackError};
use super::super::SupervisorInputs;
use super::*;
use crate::BOUNCE_PULSES;

impl LinuxCncPendantSupervisor {
    pub(super) fn cancel_pendant_motion(&mut self) {
        self.pending = None;
        if self.active.is_none() || self.phase.bounce() {
            return;
        }
        self.command = Some(CommandEvent::JogStop);
        self.transition(Phase::StoppingCancel);
    }

    fn emit_jog(&mut self, command: JogCommand, ready: bool) -> bool {
        if !ready || self.command.is_some() {
            return false;
        }
        self.command = Some(CommandEvent::JogIncrement(command));
        true
    }

    fn start_jog(&mut self, intent: JogIntent, inputs: &SupervisorInputs) -> bool {
        if !inputs.machine.ready_for_pendant_jog()
            || any(inputs.raw_limits)
            || any(inputs.safety_limits)
        {
            return false;
        }
        let start_position_pulses = match stepgen_position_pulses(
            inputs.counts_by_motor[intent.motor],
            inputs.position_feedback_by_motor[intent.motor],
        ) {
            Ok(value) => value,
            Err(StepgenFeedbackError::Unavailable) => {
                self.fail(FaultCode::JogFeedbackUnavailable);
                return false;
            }
            Err(StepgenFeedbackError::Incoherent) => {
                self.fail(FaultCode::JogFeedbackIncoherent);
                return false;
            }
        };
        let joint_jog = !inputs.machine.all_homed();
        let command = JogCommand {
            axis: intent.axis,
            joint_jog,
            signed_delta_pulses: intent.delta_pulses as f64,
            speed_mm_per_minute: intent.speed_mm_per_minute,
        };
        if !self.emit_jog(command, inputs.command_channel_ready) {
            self.pending = Some(intent);
            return false;
        }
        let start_count = inputs.counts_by_motor[intent.motor];
        self.active = Some(ActiveJog {
            intent,
            joint_jog,
            start_count,
            target_count: start_count.wrapping_add(intent.delta_pulses),
            target_position_pulses: start_position_pulses + intent.delta_pulses as f64,
        });
        self.pending = None;
        self.transition(Phase::Idle);
        self.motion_not_before_ns = MOTION_SETTLE_NS;
        true
    }

    pub(super) fn request_jog(&mut self, intent: JogIntent, inputs: &SupervisorInputs) {
        if self.phase.bounce() {
            return;
        }
        let Some(active) = self.active else {
            let _ = self.start_jog(intent, inputs);
            return;
        };

        let same_motion = self.phase == Phase::Idle
            && active.intent.motor == intent.motor
            && active.intent.axis == intent.axis
            && active.joint_jog == !inputs.machine.all_homed()
            && active.toward_positive_limit() == (intent.delta_pulses > 0);
        if same_motion && inputs.command_channel_ready {
            let current_count = inputs.counts_by_motor[intent.motor];
            let replacement_target = current_count.wrapping_add(intent.delta_pulses);
            let extension = replacement_target.wrapping_sub(active.target_count);
            if extension == 0 {
                if let Some(value) = self.active.as_mut() {
                    value.intent = intent;
                }
                self.pending = None;
                return;
            }
            if (extension > 0) == (intent.delta_pulses > 0) {
                let command = JogCommand {
                    axis: intent.axis,
                    joint_jog: active.joint_jog,
                    signed_delta_pulses: extension as f64,
                    speed_mm_per_minute: intent.speed_mm_per_minute,
                };
                if self.emit_jog(command, true) {
                    if let Some(value) = self.active.as_mut() {
                        value.intent = intent;
                        value.target_count = replacement_target;
                        value.target_position_pulses += extension as f64;
                    }
                    self.pending = None;
                    self.motion_not_before_ns = MOTION_SETTLE_NS;
                    return;
                }
            }
        }

        self.pending = Some(intent);
        if same_motion {
            return;
        }
        if self.phase != Phase::StoppingReplace {
            self.command = Some(CommandEvent::JogStop);
            self.transition(Phase::StoppingReplace);
        }
    }

    pub(super) fn begin_bounce(&mut self, motor: usize) {
        if self.active.is_none() {
            self.fail(FaultCode::UnexpectedLimit);
            return;
        }
        self.command = Some(CommandEvent::JogStopImmediate);
        self.pending = None;
        self.transition(Phase::StoppingBounce);
        self.collision_motor = Some(motor);
        self.bounce_start_count = None;
    }

    pub(super) fn start_bounce_move(&mut self, inputs: &SupervisorInputs) {
        let Some(motor) = self.collision_motor else {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        };
        if inputs.safety_limits != one_hot(motor) {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        }
        let Some(active) = self.active else {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        };
        let start_count = inputs.counts_by_motor[motor];
        let position_pulses = match stepgen_position_pulses(
            inputs.counts_by_motor[motor],
            inputs.position_feedback_by_motor[motor],
        ) {
            Ok(value) => value,
            Err(StepgenFeedbackError::Unavailable) => {
                self.fail(FaultCode::BounceFeedbackUnavailable);
                return;
            }
            Err(StepgenFeedbackError::Incoherent) => {
                self.fail(FaultCode::BounceFeedbackIncoherent);
                return;
            }
        };
        let mut fractional_phase = position_pulses - start_count as f64;
        if fractional_phase < 0.0 {
            fractional_phase = 0.0;
        }
        let below_one = f64::from_bits(1.0_f64.to_bits() - 1);
        if fractional_phase >= 1.0 {
            fractional_phase = below_one;
        }
        let distance_pulses = BOUNCE_PULSES as f64 - 0.5 + fractional_phase;
        let command = JogCommand {
            axis: axis_by_motor(motor),
            joint_jog: active.joint_jog,
            signed_delta_pulses: -distance_pulses,
            speed_mm_per_minute: BOUNCE_SPEED_MM_PER_MINUTE,
        };
        if !self.emit_jog(command, inputs.command_channel_ready) {
            return;
        }
        self.bounce_start_count = Some(start_count);
        self.transition(Phase::Bouncing);
        self.motion_not_before_ns = MOTION_SETTLE_NS;
    }

    fn finish_bounce(&mut self, inputs: &SupervisorInputs) {
        let (Some(motor), Some(start_count)) = (self.collision_motor, self.bounce_start_count)
        else {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        };
        match stepgen_position_pulses(
            inputs.counts_by_motor[motor],
            inputs.position_feedback_by_motor[motor],
        ) {
            Ok(_) => {}
            Err(StepgenFeedbackError::Unavailable) => {
                self.fail(FaultCode::BounceFeedbackUnavailable);
                return;
            }
            Err(StepgenFeedbackError::Incoherent) => {
                self.fail(FaultCode::BounceFeedbackIncoherent);
                return;
            }
        }
        let actual_delta = inputs.counts_by_motor[motor].wrapping_sub(start_count);
        if actual_delta != -BOUNCE_PULSES {
            self.fail(FaultCode::BounceCountMismatch);
            return;
        }
        if inputs.raw_limits[motor] {
            self.fail(FaultCode::BounceLimitStillActive);
            return;
        }
        self.limit_reset[motor] = true;
        self.transition(Phase::BounceResetAssert);
    }

    pub(super) fn advance_motion(&mut self, period_ns: u64, inputs: &SupervisorInputs) {
        if self.motion_not_before_ns > 0 {
            self.motion_not_before_ns = self.motion_not_before_ns.saturating_sub(period_ns);
        }
        if self.phase.bounce() {
            self.phase_total_ns = self.phase_total_ns.saturating_add(period_ns);
            if self.phase_total_ns > BOUNCE_TIMEOUT_NS {
                self.fail(FaultCode::BounceTimedOut);
                return;
            }
        }

        match self.phase {
            Phase::BounceResetAssert => {
                if self.phase_elapsed_ns < LIMIT_RESET_NS {
                    return;
                }
                self.limit_reset = [false; 3];
                self.transition(Phase::BounceResetValidate);
                return;
            }
            Phase::BounceResetValidate => {
                if self.phase_total_ns > LIMIT_RESET_TIMEOUT_NS {
                    self.fail(FaultCode::LimitLatchResetTimedOut);
                    return;
                }
                if self.phase_elapsed_ns < LIMIT_RESET_VALIDATE_NS || any(inputs.safety_limits) {
                    return;
                }
                self.active = None;
                self.pending = None;
                self.transition(Phase::Idle);
                self.collision_motor = None;
                self.bounce_start_count = None;
                self.interpreter.reset();
                return;
            }
            _ => {}
        }

        let Some(active) = self.active else {
            if self.phase == Phase::Idle && self.pending.is_some() && inputs.command_channel_ready {
                if let Some(intent) = self.pending.take() {
                    let _ = self.start_jog(intent, inputs);
                }
            }
            return;
        };
        if !inputs.command_channel_ready
            || !inputs.machine.axis_stopped[active.intent.axis.index()]
            || self.motion_not_before_ns > 0
        {
            return;
        }
        match self.phase {
            Phase::StoppingBounce => self.start_bounce_move(inputs),
            Phase::Bouncing => self.finish_bounce(inputs),
            Phase::StoppingReplace => {
                self.active = None;
                self.transition(Phase::Idle);
                if let Some(intent) = self.pending.take() {
                    let _ = self.start_jog(intent, inputs);
                }
            }
            Phase::Idle => {
                let actual_position_pulses = match stepgen_position_pulses(
                    inputs.counts_by_motor[active.intent.motor],
                    inputs.position_feedback_by_motor[active.intent.motor],
                ) {
                    Ok(value) => value,
                    Err(StepgenFeedbackError::Unavailable) => {
                        self.fail(FaultCode::JogFeedbackUnavailable);
                        return;
                    }
                    Err(StepgenFeedbackError::Incoherent) => {
                        self.fail(FaultCode::JogFeedbackIncoherent);
                        return;
                    }
                };
                let target_error = actual_position_pulses - active.target_position_pulses;
                if !target_error.is_finite()
                    || target_error <= -JOG_TARGET_TOLERANCE_PULSES
                    || target_error >= JOG_TARGET_TOLERANCE_PULSES
                {
                    self.fail(FaultCode::JogCountMismatch);
                    return;
                }
                self.active = None;
                self.transition(Phase::Idle);
                if let Some(intent) = self.pending.take() {
                    let _ = self.start_jog(intent, inputs);
                }
            }
            Phase::StoppingCancel => {
                self.active = None;
                self.pending = None;
                self.transition(Phase::Idle);
            }
            _ => {}
        }
    }

    pub(super) fn drain_pending(&mut self, inputs: &SupervisorInputs) {
        if self.phase != Phase::Idle || !inputs.command_channel_ready || self.pending.is_none() {
            return;
        }
        let intent = self.pending.take();
        if let Some(intent) = intent {
            if self.active.is_some() {
                self.request_jog(intent, inputs);
            } else {
                let _ = self.start_jog(intent, inputs);
            }
        }
    }
}
