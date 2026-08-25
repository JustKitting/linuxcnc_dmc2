use super::super::SupervisorInputs;
use super::*;

impl LinuxCncPendantSupervisor {
    pub(super) fn engage_estop(&mut self, sample: PendantSample) {
        self.command = Some(CommandEvent::JogStopImmediate);
        self.active = None;
        self.pending = None;
        self.transition(Phase::Idle);
        self.collision_motor = None;
        self.bounce_start_count = None;
        self.limit_reset = [false; 3];
        self.external_enable = false;
        self.control_available = false;
        self.control_ready = false;
        self.clear_state_requests();
        self.recovery_power_phase = None;
        self.interpreter.reset();
        self.recovery.engage(sample);
    }

    fn begin_recovery_unlock(&mut self, inputs: &SupervisorInputs) {
        if any(inputs.raw_limits) || any(inputs.safety_limits) || inputs.machine.any_homing() {
            if let Some(sample) = inputs.packet {
                let _ = self.recovery.restart(sample);
            }
            return;
        }
        self.clear_state_requests();
        self.external_enable = true;
        self.recovery_power_phase = Some(RecoveryPowerPhase::GateSettle);
        self.recovery_elapsed_ns = 0;
    }

    pub(super) fn advance_recovery(&mut self, period_ns: u64, inputs: &SupervisorInputs) {
        if inputs.link.estop_pressed {
            if let Some(sample) = inputs.packet {
                self.engage_estop(sample);
            }
            return;
        }
        if any(inputs.raw_limits) || any(inputs.safety_limits) {
            self.fail(FaultCode::LimitDuringRecovery);
            return;
        }
        self.recovery_elapsed_ns = self.recovery_elapsed_ns.saturating_add(period_ns);
        match self.recovery_power_phase {
            Some(RecoveryPowerPhase::GateSettle) if self.recovery_elapsed_ns >= GATE_SETTLE_NS => {
                self.estop_reset_request = true;
                self.machine_on_request = false;
                self.recovery_power_phase = Some(RecoveryPowerPhase::WaitReset);
                self.recovery_elapsed_ns = 0;
            }
            Some(RecoveryPowerPhase::WaitReset) if !inputs.machine.estopped => {
                self.estop_reset_request = false;
                self.machine_on_request = true;
                self.recovery_power_phase = Some(RecoveryPowerPhase::WaitOn);
                self.recovery_elapsed_ns = 0;
            }
            Some(RecoveryPowerPhase::WaitOn) if inputs.machine.machine_on => {
                self.clear_state_requests();
                self.recovery.accept_unlock();
                self.recovery_power_phase = None;
                self.interpreter.reset();
            }
            _ => {}
        }
    }

    pub(super) fn process_recovery_sample(&mut self, inputs: &SupervisorInputs) {
        if let Some(sample) = inputs.packet {
            let update = self.recovery.process(sample);
            if update.unlock_requested {
                self.begin_recovery_unlock(inputs);
            }
        }
    }
}
