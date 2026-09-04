use super::super::SupervisorInputs;
use super::*;

impl LinuxCncPendantSupervisor {
    fn begin_startup_power(&mut self) {
        self.clear_state_requests();
        self.transition(Phase::StartupReadyGateSettle);
        self.external_enable = true;
    }

    fn finish_startup_power(&mut self) {
        self.clear_state_requests();
        self.transition(Phase::Idle);
        self.interpreter.reset();
    }

    pub(super) fn advance_startup_power(&mut self, inputs: &SupervisorInputs) {
        if any(inputs.raw_limits) || any(inputs.safety_limits) {
            self.fail(FaultCode::LimitDuringStartupReset);
            return;
        }
        if inputs.machine.any_homing() {
            self.fail(FaultCode::HomingDuringStartupReset);
            return;
        }
        match self.phase {
            Phase::StartupReadyGateSettle if self.phase_elapsed_ns >= GATE_SETTLE_NS => {
                if inputs.machine.estopped {
                    self.estop_reset_request = true;
                    self.transition(Phase::StartupReadyWaitReset);
                } else if !inputs.machine.machine_on {
                    self.machine_on_request = true;
                    self.transition(Phase::StartupReadyWaitOn);
                } else {
                    self.finish_startup_power();
                }
            }
            Phase::StartupReadyWaitReset if !inputs.machine.estopped => {
                self.estop_reset_request = false;
                if !inputs.machine.machine_on {
                    self.machine_on_request = true;
                    self.transition(Phase::StartupReadyWaitOn);
                } else {
                    self.finish_startup_power();
                }
            }
            Phase::StartupReadyWaitOn if inputs.machine.machine_on => {
                self.finish_startup_power();
            }
            _ => {}
        }
    }

    fn begin_startup_bounce(&mut self, motor: usize) {
        self.clear_state_requests();
        self.active = None;
        self.pending = None;
        self.collision_motor = Some(motor);
        self.bounce_start_count = None;
        self.transition(Phase::StartupGateSettle);
        self.external_enable = true;
    }

    pub(super) fn check_startup_limits(&mut self, inputs: &SupervisorInputs) {
        if self.startup_limits_checked {
            return;
        }
        self.startup_limits_checked = true;
        if !any(inputs.raw_limits) && !any(inputs.safety_limits) {
            if inputs.machine.estopped || !inputs.machine.machine_on {
                self.begin_startup_power();
            } else {
                self.clear_state_requests();
            }
            return;
        }
        let raw_motor = single_active(inputs.raw_limits);
        let safety_motor = single_active(inputs.safety_limits);
        if raw_motor.is_none() || (safety_motor.is_some() && safety_motor != raw_motor) {
            self.fail(FaultCode::StartupLimitMismatch);
            return;
        }
        if inputs.machine.any_homing() {
            self.fail(FaultCode::StartupLimitDuringHoming);
            return;
        }
        self.begin_startup_bounce(raw_motor.unwrap_or(0));
    }

    pub(super) fn advance_startup_bounce(&mut self, inputs: &SupervisorInputs) {
        let Some(motor) = self.collision_motor else {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        };
        let expected = one_hot(motor);
        if any(inputs.safety_limits) && inputs.safety_limits != expected {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        }
        if inputs.safety_limits != expected {
            return;
        }
        match self.phase {
            Phase::StartupGateSettle if self.phase_elapsed_ns >= GATE_SETTLE_NS => {
                if inputs.machine.estopped {
                    self.estop_reset_request = true;
                    self.transition(Phase::StartupWaitReset);
                    return;
                }
                if !inputs.machine.machine_on {
                    self.machine_on_request = true;
                    self.transition(Phase::StartupWaitOn);
                    return;
                }
            }
            Phase::StartupWaitReset if inputs.machine.estopped => return,
            Phase::StartupWaitReset => {
                self.estop_reset_request = false;
                if !inputs.machine.machine_on {
                    self.machine_on_request = true;
                    self.transition(Phase::StartupWaitOn);
                    return;
                }
            }
            Phase::StartupWaitOn if !inputs.machine.machine_on => return,
            Phase::StartupWaitOn => self.machine_on_request = false,
            _ => {}
        }
        let Some(path) = inputs.ready_jog_path() else {
            return;
        };
        self.clear_state_requests();
        self.start_bounce_move(inputs, path);
    }
}
