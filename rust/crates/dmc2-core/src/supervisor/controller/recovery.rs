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
        self.recovery_restore_machine_on = false;
        self.interpreter.reset();
        self.recovery.engage(sample);
    }

    fn begin_recovery_unlock(&mut self, inputs: &SupervisorInputs) {
        if any(inputs.raw_limits) || any(inputs.safety_limits) || inputs.machine.any_homing() {
            if let Some(sample) = inputs.packet {
                self.recovery.restart(sample);
            }
            return;
        }
        self.clear_state_requests();
        self.external_enable = true;
        self.recovery_power_phase = Some(RecoveryPowerPhase::GateSettle);
        self.recovery_restore_machine_on = true;
        self.recovery_elapsed_ns = 0;
    }

    /// Rejoin the controller gate to LinuxCNC's canonical E-stop latch.
    /// Unlike the pendant recovery gesture, a base AXIS reset does not also
    /// request Machine On.
    pub fn begin_linuxcnc_estop_reset(&mut self) {
        self.clear_state_requests();
        self.external_enable = true;
        self.recovery.accept_unlock();
        self.recovery_power_phase = Some(RecoveryPowerPhase::GateSettle);
        self.recovery_restore_machine_on = false;
        self.recovery_elapsed_ns = 0;
        self.interpreter.reset();
    }

    pub(super) fn accept_linuxcnc_estop_reset(&mut self, inputs: &SupervisorInputs) {
        if any(inputs.raw_limits) || any(inputs.safety_limits) || inputs.machine.any_homing() {
            return;
        }
        self.begin_linuxcnc_estop_reset();
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
                if self.recovery_restore_machine_on {
                    self.machine_on_request = true;
                    self.recovery_power_phase = Some(RecoveryPowerPhase::WaitOn);
                } else {
                    self.clear_state_requests();
                    self.recovery.accept_unlock();
                    self.recovery_power_phase = None;
                    self.interpreter.reset();
                }
                self.recovery_elapsed_ns = 0;
            }
            Some(RecoveryPowerPhase::WaitOn) if inputs.machine.machine_on => {
                self.clear_state_requests();
                self.recovery.accept_unlock();
                self.recovery_power_phase = None;
                self.recovery_restore_machine_on = false;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pendant::{AxisSelector, MultiplierSelector};
    use crate::supervisor::MotionSnapshot;

    fn sample(sequence: u32, estop_pressed: bool) -> PendantSample {
        PendantSample {
            sequence,
            quadrature_errors: 0,
            latest_detent: 0,
            axis: AxisSelector::X,
            multiplier: MultiplierSelector::X1,
            deadman_held: false,
            estop_pressed,
            selector_valid: true,
        }
    }

    fn inputs(
        packet: PendantSample,
        machine_on: bool,
        estopped: bool,
        linuxcnc_estop_reset_rising: bool,
    ) -> SupervisorInputs {
        SupervisorInputs {
            link: LinkSnapshot {
                connected: true,
                serial_fault: false,
                quadrature_fault: false,
                estop_pressed: packet.estop_pressed,
            },
            packet: Some(packet),
            machine: MachineSnapshot {
                machine_on,
                estopped,
                manual_mode: true,
                joint_mode: true,
                teleop_mode: false,
                interp_idle: true,
                homed: [false; 3],
                homing: [false; 3],
                axis_stopped: [true; 3],
            },
            motion: MotionSnapshot {
                enabled: false,
                teleop_mode: false,
                coord_mode: false,
                in_position: true,
                jog_active: false,
                axis_wheel_jog_active: [false; 3],
                joint_wheel_jog_active: [false; 3],
                joint_in_position: [true; 3],
            },
            counts_by_motor: [0; 3],
            position_feedback_by_motor: [0.0; 3],
            raw_limits: [false; 3],
            safety_limits: [false; 3],
            pendant_mode_enabled: false,
            motion_command_ready: true,
            linuxcnc_estop_reset_rising,
        }
    }

    #[test]
    fn pendant_press_drops_the_linuxcnc_gate_immediately() {
        let mut supervisor = LinuxCncPendantSupervisor::new();
        let output = supervisor.update(1_000_000, inputs(sample(1, true), false, true, false));

        assert!(!output.external_enable);
        assert!(output.recovery_active);
        assert!(!output.estop_reset_request);
        assert!(!output.machine_on_request);
    }

    #[test]
    fn base_reset_clears_the_same_recovery_without_turning_machine_on() {
        let mut supervisor = LinuxCncPendantSupervisor::new();
        supervisor.update(1_000_000, inputs(sample(1, true), false, true, false));

        let released = supervisor.update(1_000_000, inputs(sample(2, false), false, true, true));
        assert!(released.external_enable);
        assert!(released.recovery_active);
        assert!(!released.machine_on_request);

        let reset_request =
            supervisor.update(GATE_SETTLE_NS, inputs(sample(3, false), false, true, false));
        assert!(reset_request.estop_reset_request);
        assert!(!reset_request.machine_on_request);

        let reset_acknowledged =
            supervisor.update(1_000_000, inputs(sample(4, false), false, false, false));
        assert!(reset_acknowledged.external_enable);
        assert!(!reset_acknowledged.recovery_active);
        assert!(!reset_acknowledged.estop_reset_request);
        assert!(!reset_acknowledged.machine_on_request);
    }

    #[test]
    fn base_reset_cannot_clear_while_physical_estop_is_pressed() {
        let mut supervisor = LinuxCncPendantSupervisor::new();
        supervisor.update(1_000_000, inputs(sample(1, true), false, true, false));

        let output = supervisor.update(1_000_000, inputs(sample(2, true), false, true, true));

        assert!(!output.external_enable);
        assert!(output.recovery_active);
        assert!(!output.estop_reset_request);
        assert!(!output.machine_on_request);
    }

    #[test]
    fn pendant_recovery_still_restores_machine_on_after_reset() {
        let mut supervisor = LinuxCncPendantSupervisor::new();
        supervisor.update(1_000_000, inputs(sample(1, true), false, true, false));
        let released_inputs = inputs(sample(2, false), false, true, false);
        supervisor.begin_recovery_unlock(&released_inputs);

        let reset_request = supervisor.update(GATE_SETTLE_NS, released_inputs);
        assert!(reset_request.estop_reset_request);

        // The request returns through HALUI and iocontrol as LinuxCNC's same
        // canonical base-reset edge. It acknowledges the request; it must not
        // replace the already selected pendant recovery policy.
        let reset_loopback =
            supervisor.update(1_000_000, inputs(sample(3, false), false, true, true));
        assert!(reset_loopback.estop_reset_request);
        assert!(!reset_loopback.machine_on_request);

        let reset_acknowledged =
            supervisor.update(1_000_000, inputs(sample(4, false), false, false, false));
        assert!(reset_acknowledged.machine_on_request);

        let on_acknowledged =
            supervisor.update(1_000_000, inputs(sample(5, false), true, false, false));
        assert!(!on_acknowledged.recovery_active);
        assert!(!on_acknowledged.machine_on_request);
    }
}
