use super::super::feedback::{stepgen_position_pulses, StepgenFeedbackContext};
use super::super::SupervisorInputs;
use super::*;
use crate::BOUNCE_PULSES;

impl LinuxCncPendantSupervisor {
    pub(super) fn cancel_pendant_motion(&mut self) {
        self.pending = None;
        if self.active.is_none() {
            return;
        }
        // Cancellation is level-driven while LinuxCNC reports homing.  Once
        // the stop transition has begun, do not republish motion.jog-stop on
        // every servo cycle while the planner decelerates and settles.
        if self.phase == Phase::StoppingCancel {
            return;
        }
        if self.phase == Phase::BounceReleaseJog {
            self.command = Some(CommandEvent::JogStop);
            self.transition(Phase::BounceReleaseStopping);
            return;
        }
        if self.phase.bounce() {
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

    fn start_jog(&mut self, intent: JogIntent, inputs: &SupervisorInputs) {
        if any(inputs.raw_limits) || any(inputs.safety_limits) {
            return;
        }
        let Some(path) = inputs.ready_jog_path() else {
            return;
        };
        let start_position_pulses = match stepgen_position_pulses(
            inputs.counts_by_motor[intent.motor],
            inputs.position_feedback_by_motor[intent.motor],
            StepgenFeedbackContext::Jog,
        ) {
            Ok(value) => value,
            Err(error) => {
                self.fail(error.fault_code());
                return;
            }
        };
        let command = JogCommand {
            axis: intent.axis,
            path,
            signed_delta_pulses: intent.delta_pulses as f64,
            target_rate_mm_per_minute: intent.target_rate_mm_per_minute,
        };
        if !self.emit_jog(command, inputs.motion_command_ready) {
            self.pending = Some(intent);
            return;
        }
        let start_count = inputs.counts_by_motor[intent.motor];
        self.active = Some(ActiveJog {
            intent,
            path,
            start_count,
            start_position_pulses,
            target_count: start_count.wrapping_add(intent.delta_pulses),
            target_position_pulses: start_position_pulses + intent.delta_pulses as f64,
            command_elapsed_ns: 0,
            consumer_active_seen: false,
            feedback_progress_seen: false,
        });
        self.pending = None;
        self.transition(Phase::Idle);
    }

    pub(super) fn request_jog(&mut self, intent: JogIntent, inputs: &SupervisorInputs) {
        if self.phase.bounce() {
            return;
        }
        let Some(active) = self.active else {
            self.start_jog(intent, inputs);
            return;
        };

        let same_motion = self.phase == Phase::Idle
            && active.intent.motor == intent.motor
            && active.intent.axis == intent.axis
            && inputs.path_ready(active.path)
            && active.toward_positive_limit() == (intent.delta_pulses > 0);
        if same_motion && inputs.motion_command_ready {
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
                    path: active.path,
                    signed_delta_pulses: extension as f64,
                    target_rate_mm_per_minute: intent.target_rate_mm_per_minute,
                };
                if self.emit_jog(command, true) {
                    if let Some(value) = self.active.as_mut() {
                        value.intent = intent;
                        value.target_count = replacement_target;
                        value.target_position_pulses += extension as f64;
                        value.command_elapsed_ns = 0;
                    }
                    self.pending = None;
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

    pub(super) fn start_bounce_move(&mut self, inputs: &SupervisorInputs, path: JogPath) {
        let Some(motor) = self.collision_motor else {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        };
        if inputs.safety_limits != one_hot(motor) {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        }
        if !inputs.path_ready(path) {
            self.fail(FaultCode::MotionPathUnavailable);
            return;
        }
        let start_count = inputs.counts_by_motor[motor];
        let position_pulses = match stepgen_position_pulses(
            inputs.counts_by_motor[motor],
            inputs.position_feedback_by_motor[motor],
            StepgenFeedbackContext::LimitRecovery,
        ) {
            Ok(value) => value,
            Err(error) => {
                self.fail(error.fault_code());
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
            path,
            signed_delta_pulses: -distance_pulses,
            target_rate_mm_per_minute: BOUNCE_SPEED_MM_PER_MINUTE,
        };
        if !self.emit_jog(command, inputs.motion_command_ready) {
            return;
        }
        self.bounce_start_count = Some(start_count);
        let intent = JogIntent {
            axis: axis_by_motor(motor),
            motor,
            delta_pulses: -BOUNCE_PULSES,
            target_rate_mm_per_minute: BOUNCE_SPEED_MM_PER_MINUTE,
        };
        if let Some(value) = self.active.as_mut() {
            value.intent = intent;
            value.path = path;
            value.restart_observation(
                start_count,
                position_pulses,
                start_count.wrapping_sub(BOUNCE_PULSES),
                position_pulses - distance_pulses,
            );
        } else {
            self.active = Some(ActiveJog {
                intent,
                path,
                start_count,
                start_position_pulses: position_pulses,
                target_count: start_count.wrapping_sub(BOUNCE_PULSES),
                target_position_pulses: position_pulses - distance_pulses,
                command_elapsed_ns: 0,
                consumer_active_seen: false,
                feedback_progress_seen: false,
            });
        }
        self.transition(Phase::Bouncing);
    }

    pub(super) fn start_limit_release_jog(&mut self, intent: JogIntent, inputs: &SupervisorInputs) {
        let Some(motor) = self.collision_motor else {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        };
        if self.phase != Phase::BounceReleaseWait
            || intent.motor != motor
            || intent.axis != axis_by_motor(motor)
            || intent.delta_pulses >= 0
        {
            return;
        }
        if !inputs.raw_limits[motor] {
            self.begin_limit_reset(motor);
            return;
        }
        if inputs.safety_limits != one_hot(motor) {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        }
        let Some(path) = inputs.ready_jog_path() else {
            return;
        };
        let start_position_pulses = match stepgen_position_pulses(
            inputs.counts_by_motor[motor],
            inputs.position_feedback_by_motor[motor],
            StepgenFeedbackContext::LimitRecovery,
        ) {
            Ok(value) => value,
            Err(error) => {
                self.fail(error.fault_code());
                return;
            }
        };
        let command = JogCommand {
            axis: intent.axis,
            path,
            signed_delta_pulses: intent.delta_pulses as f64,
            target_rate_mm_per_minute: intent.target_rate_mm_per_minute,
        };
        if !self.emit_jog(command, inputs.motion_command_ready) {
            return;
        }
        let start_count = inputs.counts_by_motor[motor];
        self.active = Some(ActiveJog {
            intent,
            path,
            start_count,
            start_position_pulses,
            target_count: start_count.wrapping_add(intent.delta_pulses),
            target_position_pulses: start_position_pulses + intent.delta_pulses as f64,
            command_elapsed_ns: 0,
            consumer_active_seen: false,
            feedback_progress_seen: false,
        });
        self.pending = None;
        self.transition(Phase::BounceReleaseJog);
    }

    fn begin_limit_reset(&mut self, motor: usize) {
        self.active = None;
        self.pending = None;
        self.limit_reset[motor] = true;
        self.transition(Phase::BounceResetAssert);
    }

    fn finish_unaccepted_manual_jog(&mut self) {
        // LinuxCNC normally refuses an outward wheel increment at a Cartesian
        // soft limit. No motion began, so this is an ordinary bounded manual
        // input outcome rather than a machine fault or E-stop condition.
        self.active = None;
        self.pending = None;
        // A skipped limit-release increment retains attribution and the
        // operator's existing away-only control path; it does not retry.
        self.transition(if self.phase == Phase::BounceReleaseJog {
            Phase::BounceReleaseWait
        } else {
            Phase::Idle
        });
        self.interpreter.reset();
    }

    fn finish_bounce(&mut self, inputs: &SupervisorInputs) {
        if self.phase == Phase::Bouncing && self.bounce_start_count.is_none() {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        }
        let Some(motor) = self.collision_motor else {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        };
        let Some(active) = self.active else {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        };
        let actual_position_pulses = match stepgen_position_pulses(
            inputs.counts_by_motor[motor],
            inputs.position_feedback_by_motor[motor],
            StepgenFeedbackContext::LimitRecovery,
        ) {
            Ok(value) => value,
            Err(error) => {
                self.fail(error.fault_code());
                return;
            }
        };
        let target_error = actual_position_pulses - active.target_position_pulses;
        if !manual_target_reached(target_error, active.intent.delta_pulses) {
            return;
        }
        if inputs.raw_limits[motor] {
            self.active = None;
            self.pending = None;
            self.interpreter.reset();
            self.transition(Phase::BounceReleaseWait);
            return;
        }
        self.begin_limit_reset(motor);
    }

    pub(super) fn advance_motion(&mut self, period_ns: u64, inputs: &SupervisorInputs) {
        if self.phase.timed_bounce() {
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

        if self.phase == Phase::BounceReleaseWait {
            let Some(motor) = self.collision_motor else {
                self.fail(FaultCode::BounceLostLimitAttribution);
                return;
            };
            if !inputs.raw_limits[motor] {
                self.begin_limit_reset(motor);
            }
            return;
        }

        let Some(active) = self.active else {
            if self.phase == Phase::Idle && self.pending.is_some() && inputs.motion_command_ready {
                if let Some(intent) = self.pending.take() {
                    self.start_jog(intent, inputs);
                }
            }
            return;
        };
        if let Some(value) = self.active.as_mut() {
            value.observe_motion(period_ns, inputs);
        }
        let active = self.active.unwrap_or(active);

        match self.phase {
            Phase::StoppingBounce
            | Phase::StoppingReplace
            | Phase::StoppingCancel
            | Phase::BounceReleaseStopping => {
                if self.phase_elapsed_ns >= MOTION_STOP_TIMEOUT_NS {
                    self.fail(FaultCode::MotionStopTimedOut);
                    return;
                }
                if !inputs.motion.all_jogs_stopped()
                    || !inputs.motion.path_settled(active.intent.axis, active.path)
                {
                    return;
                }
                match self.phase {
                    Phase::StoppingBounce => self.start_bounce_move(inputs, active.path),
                    Phase::StoppingReplace => {
                        self.active = None;
                        self.transition(Phase::Idle);
                        if let Some(intent) = self.pending.take() {
                            self.start_jog(intent, inputs);
                        }
                    }
                    Phase::StoppingCancel => {
                        self.active = None;
                        self.pending = None;
                        self.transition(Phase::Idle);
                    }
                    Phase::BounceReleaseStopping => {
                        self.active = None;
                        self.pending = None;
                        self.interpreter.reset();
                        self.transition(Phase::BounceReleaseWait);
                    }
                    _ => {}
                }
            }
            Phase::Bouncing | Phase::BounceReleaseJog => {
                if !inputs.path_ready(active.path) {
                    if self.phase == Phase::BounceReleaseJog {
                        self.cancel_pendant_motion();
                    } else {
                        self.fail(FaultCode::MotionPathUnavailable);
                    }
                    return;
                }
                if !active.consumer_active_seen {
                    if active.command_elapsed_ns >= MOTION_ACCEPT_TIMEOUT_NS {
                        if self.phase == Phase::BounceReleaseJog {
                            self.finish_unaccepted_manual_jog();
                        } else {
                            self.fail(FaultCode::JogCommandNotAccepted);
                        }
                    }
                    return;
                }
                if !inputs.motion.path_settled(active.intent.axis, active.path) {
                    if active.command_elapsed_ns >= BOUNCE_TIMEOUT_NS {
                        self.fail(FaultCode::BounceTimedOut);
                    }
                    return;
                }
                self.finish_bounce(inputs);
                if matches!(self.phase, Phase::Bouncing | Phase::BounceReleaseJog)
                    && active.command_elapsed_ns >= BOUNCE_TIMEOUT_NS
                {
                    self.fail(FaultCode::BounceCountMismatch);
                }
            }
            Phase::Idle => {
                if !inputs.path_ready(active.path) {
                    // Task and realtime motion state can cross between free and
                    // teleop modes on different observations.  A pendant
                    // increment at that boundary is discarded through the
                    // existing controlled-stop path; it is not a machine
                    // fault and never latches recovery.
                    self.cancel_pendant_motion();
                    return;
                }
                let actual_position_pulses = match stepgen_position_pulses(
                    inputs.counts_by_motor[active.intent.motor],
                    inputs.position_feedback_by_motor[active.intent.motor],
                    StepgenFeedbackContext::Jog,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        self.fail(error.fault_code());
                        return;
                    }
                };
                if !active.consumer_active_seen {
                    if active.command_elapsed_ns >= MOTION_ACCEPT_TIMEOUT_NS {
                        self.finish_unaccepted_manual_jog();
                    }
                    return;
                }
                if !inputs.motion.path_settled(active.intent.axis, active.path) {
                    if active.command_elapsed_ns >= JOG_TIMEOUT_NS {
                        self.fail(FaultCode::JogTimedOut);
                    }
                    return;
                }
                let target_error = actual_position_pulses - active.target_position_pulses;
                if !manual_target_reached(target_error, active.intent.delta_pulses) {
                    if active.command_elapsed_ns >= JOG_TIMEOUT_NS {
                        self.fail(FaultCode::JogCountMismatch);
                    }
                    return;
                }
                self.active = None;
                self.transition(Phase::Idle);
                if let Some(intent) = self.pending.take() {
                    self.start_jog(intent, inputs);
                }
            }
            _ => {}
        }
    }

    pub(super) fn drain_pending(&mut self, inputs: &SupervisorInputs) {
        if self.phase != Phase::Idle || !inputs.motion_command_ready || self.pending.is_none() {
            return;
        }
        let intent = self.pending.take();
        if let Some(intent) = intent {
            if self.active.is_some() {
                self.request_jog(intent, inputs);
            } else {
                self.start_jog(intent, inputs);
            }
        }
    }
}
