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
    pub fn begin_linuxcnc_estop_reset(&mut self, inputs: &SupervisorInputs) {
        self.clear_state_requests();
        self.startup_progress = StartupProgress::Consumed;
        self.external_enable = false;
        self.recovery.accept_unlock();
        self.recovery_restore_machine_on = false;
        self.recovery_elapsed_ns = 0;
        self.interpreter.reset();
        // This explicit reset may release stale latches, never asserted raw
        // switches. A unique remaining switch uses the existing manual-only
        // away-jog state, not another automatic startup backoff.
        self.collision_motor = single_active(inputs.raw_limits);
        if any(inputs.raw_limits) && self.collision_motor.is_none() {
            self.fail(FaultCode::LimitDuringRecovery);
            return;
        }
        self.transition(if self.collision_motor.is_some() {
            Phase::BounceReleaseWait
        } else {
            Phase::Idle
        });
        let stale =
            core::array::from_fn(|motor| inputs.safety_limits[motor] && !inputs.raw_limits[motor]);
        if any(stale) {
            self.limit_reset = stale;
            self.recovery_power_phase = Some(RecoveryPowerPhase::LimitResetAssert(stale));
        } else {
            self.external_enable = true;
            self.recovery_power_phase = Some(RecoveryPowerPhase::GateSettle);
        }
    }

    pub(super) fn accept_linuxcnc_estop_reset(&mut self, inputs: &SupervisorInputs) {
        self.begin_linuxcnc_estop_reset(inputs);
    }

    pub(super) fn advance_recovery(&mut self, period_ns: u64, inputs: &SupervisorInputs) {
        if inputs.link.estop_pressed {
            if let Some(sample) = inputs.packet {
                self.engage_estop(sample);
            }
            return;
        }
        if self.recovery_power_phase == Some(RecoveryPowerPhase::AwaitUiReset) {
            self.external_enable = false;
            if inputs.linuxcnc_estop_reset_rising {
                self.begin_linuxcnc_estop_reset(inputs);
            }
            return;
        }
        let attributed = self.collision_motor.map_or([false; 3], one_hot);
        let resetting = match self.recovery_power_phase {
            Some(
                RecoveryPowerPhase::LimitResetAssert(mask)
                | RecoveryPowerPhase::LimitResetValidate(mask),
            ) => mask,
            _ => [false; 3],
        };
        if (0..3).any(|motor| {
            (inputs.raw_limits[motor] && !attributed[motor])
                || (inputs.safety_limits[motor] && !attributed[motor] && !resetting[motor])
        }) {
            self.fail(FaultCode::LimitDuringRecovery);
            return;
        }
        self.recovery_elapsed_ns = self.recovery_elapsed_ns.saturating_add(period_ns);
        match self.recovery_power_phase {
            Some(RecoveryPowerPhase::LimitResetAssert(mask))
                if self.recovery_elapsed_ns >= LIMIT_RESET_NS =>
            {
                self.limit_reset = [false; 3];
                self.recovery_power_phase = Some(RecoveryPowerPhase::LimitResetValidate(mask));
                self.recovery_elapsed_ns = 0;
            }
            Some(RecoveryPowerPhase::LimitResetValidate(mask)) => {
                if self.recovery_elapsed_ns >= LIMIT_RESET_TIMEOUT_NS {
                    self.fail(FaultCode::LimitLatchResetTimedOut);
                } else if self.recovery_elapsed_ns >= LIMIT_RESET_VALIDATE_NS
                    && !(0..3).any(|motor| mask[motor] && inputs.safety_limits[motor])
                {
                    self.external_enable = true;
                    self.recovery_power_phase = Some(RecoveryPowerPhase::GateSettle);
                    self.recovery_elapsed_ns = 0;
                }
            }
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

    fn faulted_reset(
        raw: [bool; 3],
        safety: [bool; 3],
    ) -> (LinuxCncPendantSupervisor, SupervisorInputs) {
        let mut supervisor = LinuxCncPendantSupervisor::new();
        let mut frame = inputs(sample(1, false), false, true, false);
        supervisor.update(1_000_000, frame);
        supervisor.fail(FaultCode::UnexpectedLimit);
        supervisor.clear_latched_fault();
        assert!(supervisor.fault().is_none());
        frame.raw_limits = raw;
        frame.safety_limits = safety;
        supervisor.begin_linuxcnc_estop_reset(&frame);
        (supervisor, frame)
    }

    #[test]
    fn ui_reset_releases_each_stale_latch_mask_without_power_or_motion() {
        for mask in 1..8 {
            let stale = core::array::from_fn(|motor| mask & (1 << motor) != 0);
            let (mut supervisor, mut frame) = faulted_reset([false; 3], stale);
            assert_eq!(supervisor.outputs().limit_reset, stale);
            for cycle in 1..120 {
                frame.packet = Some(sample(cycle + 1, false));
                frame.machine.estopped = cycle < 80;
                if cycle >= 10 {
                    frame.safety_limits = [false; 3];
                }
                let output = supervisor.update(1_000_000, frame);
                assert!(output.fault.is_none());
                assert!(!output.machine_on_request);
                assert!(!matches!(
                    output.command,
                    Some(CommandEvent::JogIncrement(_))
                ));
            }
            assert!(!supervisor.outputs().recovery_active);
            assert_eq!(supervisor.outputs().limit_reset, [false; 3]);
        }
    }

    #[test]
    fn new_raw_limit_immediately_cancels_stale_latch_reset() {
        let (mut supervisor, mut frame) = faulted_reset([false; 3], [true, false, false]);
        frame.raw_limits[0] = true;
        let output = supervisor.update(1_000_000, frame);
        assert_eq!(output.fault, Some(FaultCode::LimitDuringRecovery));
        assert_eq!(output.limit_reset, [false; 3]);
        assert!(!output.external_enable);
    }

    #[test]
    fn ui_limit_recovery_preserves_manual_release_after_an_unaccepted_increment() {
        let (mut supervisor, mut frame) = faulted_reset([false, true, false], [true; 3]);
        assert_eq!(supervisor.outputs().limit_reset, [true, false, true]);
        for cycle in 1..120 {
            frame.packet = Some(sample(cycle + 1, false));
            frame.machine.estopped = cycle < 80;
            if cycle >= 10 {
                frame.safety_limits = [false, true, false];
            }
            let output = supervisor.update(1_000_000, frame);
            assert!(output.fault.is_none());
            assert!(!output.machine_on_request);
            assert!(!matches!(
                output.command,
                Some(CommandEvent::JogIncrement(_))
            ));
        }
        assert_eq!(supervisor.outputs().phase, Phase::BounceReleaseWait);
        frame.machine.machine_on = true;
        frame.motion.enabled = true;
        frame.pendant_mode_enabled = true;
        let away_detent = -Axis::X.clockwise_machine_sign();
        for (offset, latest_detent) in [0, 0, away_detent].into_iter().enumerate() {
            frame.packet = Some(PendantSample {
                latest_detent,
                deadman_held: true,
                ..sample(200 + offset as u32, false)
            });
            supervisor.update(1_000_000, frame);
        }
        assert_eq!(supervisor.outputs().phase, Phase::BounceReleaseJog);
        for sequence in 203..350 {
            frame.packet = Some(PendantSample {
                deadman_held: true,
                ..sample(sequence, false)
            });
            let output = supervisor.update(1_000_000, frame);
            assert!(output.fault.is_none());
            assert!(output.external_enable);
            assert!(!output.recovery_active);
        }
        assert_eq!(supervisor.outputs().phase, Phase::BounceReleaseWait);
        assert_eq!(supervisor.collision_motor, Some(1));
        assert!(!supervisor.outputs().jog_active);
    }
}
