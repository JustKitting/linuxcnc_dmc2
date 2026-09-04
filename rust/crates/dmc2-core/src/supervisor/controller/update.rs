use super::super::{SupervisorInputs, SupervisorOutputs};
use super::*;
use crate::pendant::{InterpreterFault, PendantDecision};

impl LinuxCncPendantSupervisor {
    fn set_pendant_mode(&mut self, enabled: bool) {
        if enabled == self.pendant_mode_enabled {
            return;
        }
        self.pendant_mode_enabled = enabled;
        self.interpreter.reset();
        self.pending = None;
        if !enabled {
            self.cancel_pendant_motion();
        }
    }

    fn process_limit_release_sample(&mut self, inputs: &SupervisorInputs) {
        let Some(sample) = inputs.packet else {
            return;
        };
        match self.interpreter.process(sample) {
            PendantDecision::Stop(_) => self.cancel_pendant_motion(),
            PendantDecision::NoDetent => {}
            PendantDecision::Jog(intent) => {
                if self.phase == Phase::BounceReleaseWait {
                    self.start_limit_release_jog(intent, inputs);
                }
            }
            PendantDecision::Fault(
                InterpreterFault::QuadratureErrorChanged | InterpreterFault::InvalidDetent,
            ) => self.fail(FaultCode::InvalidPendantPacket),
        }
    }

    pub fn update(&mut self, period_ns: u64, inputs: SupervisorInputs) -> SupervisorOutputs {
        self.observe_inputs(inputs);
        self.command = None;
        self.phase_elapsed_ns = self.phase_elapsed_ns.saturating_add(period_ns);
        self.set_pendant_mode(inputs.pendant_mode_enabled);
        self.control_ready = false;

        if inputs.packet.is_some() {
            self.link_established = true;
        }
        if self.fault.is_some() {
            self.external_enable = false;
            return self.outputs();
        }
        if !self.link_established {
            self.external_enable = false;
            return self.outputs();
        }
        // A physical pendant E-stop always owns this transition. It drops the
        // same external gate consumed by LinuxCNC's estop_latch and cannot be
        // delayed behind diagnostic classification.
        if inputs.link.estop_pressed {
            if let Some(sample) = inputs.packet {
                if !self.recovery.active() {
                    self.engage_estop(sample);
                }
            }
            self.external_enable = false;
            return self.outputs();
        }
        if inputs.linuxcnc_estop_reset_rising
            && self.recovery.active()
            && self.recovery_power_phase.is_none()
        {
            self.accept_linuxcnc_estop_reset(&inputs);
        }
        if self.recovery_power_phase.is_some() {
            self.advance_recovery(period_ns, &inputs);
            return self.outputs();
        }
        if self.recovery.active() {
            self.external_enable = false;
            self.process_recovery_sample(&inputs);
            return self.outputs();
        }
        if inputs.link.serial_fault || !inputs.link.connected {
            self.fail(FaultCode::LinkFailure);
            return self.outputs();
        }
        if inputs.link.quadrature_fault {
            self.fail(FaultCode::QuadratureFailure);
            return self.outputs();
        }

        self.check_startup_limits(&inputs);
        if self.fault.is_some() {
            return self.outputs();
        }
        if self.phase.startup_power() {
            self.external_enable = true;
            self.advance_startup_power(&inputs);
            return self.outputs();
        }
        if self.phase.startup_bounce() {
            self.external_enable = true;
            self.check_limits(&inputs);
            if self.fault.is_none() {
                self.advance_startup_bounce(&inputs);
            }
            return self.outputs();
        }

        self.external_enable = true;
        self.manage_homing_latches(period_ns, &inputs);
        self.check_limits(&inputs);
        if self.fault.is_none() {
            self.advance_motion(period_ns, &inputs);
            self.drain_pending(&inputs);
        }
        if self.fault.is_some() {
            return self.outputs();
        }

        if self.pendant_mode_enabled {
            if matches!(
                self.phase,
                Phase::BounceReleaseWait | Phase::BounceReleaseJog
            ) {
                self.process_limit_release_sample(&inputs);
            } else if !self.phase.bounce() {
                if let Some(sample) = inputs.packet {
                    match self.interpreter.process(sample) {
                        PendantDecision::Stop(_) => self.cancel_pendant_motion(),
                        PendantDecision::NoDetent => {}
                        PendantDecision::Jog(intent) => self.request_jog(intent, &inputs),
                        PendantDecision::Fault(
                            InterpreterFault::QuadratureErrorChanged
                            | InterpreterFault::InvalidDetent,
                        ) => self.fail(FaultCode::InvalidPendantPacket),
                    }
                }
            }
        }
        self.control_ready = self.external_enable
            && self.startup_sequence_complete()
            && self.recovery_power_phase.is_none()
            && inputs.ready_jog_path().is_some()
            && self.pendant_mode_enabled
            && !self.phase.bounce()
            && !any(inputs.raw_limits)
            && !any(inputs.safety_limits);
        self.outputs()
    }
}
