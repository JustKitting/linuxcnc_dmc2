use super::super::SupervisorInputs;
use super::*;

impl LinuxCncPendantSupervisor {
    pub(super) fn check_limits(&mut self, inputs: &SupervisorInputs) {
        if !any(inputs.safety_limits) || inputs.machine.any_homing() || self.homing_was_active {
            return;
        }
        let active_motor = single_active(inputs.safety_limits);
        if self.phase.bounce() {
            if active_motor != self.collision_motor {
                self.fail(FaultCode::BounceLostLimitAttribution);
            }
            return;
        }
        if let (Some(active), Some(motor)) = (self.active, active_motor) {
            if active.toward_positive_limit() && active.intent.motor == motor {
                self.begin_bounce(motor);
                return;
            }
        }
        self.fail(FaultCode::UnexpectedLimit);
    }

    pub(super) fn manage_homing_latches(&mut self, period_ns: u64, inputs: &SupervisorInputs) {
        if inputs.machine.any_homing() {
            self.homing_was_active = true;
            self.homing_reset_elapsed_ns = 0;
            self.cancel_pendant_motion();
            return;
        }
        if !self.homing_was_active || any(inputs.raw_limits) {
            return;
        }
        if !any(self.limit_reset) && any(inputs.safety_limits) {
            self.limit_reset = inputs.safety_limits;
            self.homing_reset_elapsed_ns = 0;
            return;
        }
        if any(self.limit_reset) {
            self.homing_reset_elapsed_ns = self.homing_reset_elapsed_ns.saturating_add(period_ns);
            if self.homing_reset_elapsed_ns >= LIMIT_RESET_NS {
                self.limit_reset = [false; 3];
                self.homing_reset_elapsed_ns = 0;
            }
            return;
        }
        if !any(inputs.safety_limits) {
            self.homing_was_active = false;
            self.homing_reset_elapsed_ns = 0;
        }
    }
}
