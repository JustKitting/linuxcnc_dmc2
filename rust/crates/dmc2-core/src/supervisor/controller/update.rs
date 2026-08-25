use super::super::{SupervisorInputs, SupervisorOutputs};
use super::*;
use crate::pendant::PendantDecision;

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

    pub fn update(&mut self, period_ns: u64, inputs: SupervisorInputs) -> SupervisorOutputs {
        self.command = None;
        self.phase_elapsed_ns = self.phase_elapsed_ns.saturating_add(period_ns);
        self.set_pendant_mode(inputs.pendant_mode_enabled);
        self.control_available = false;
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
        if inputs.link.serial_fault || !inputs.link.connected {
            self.fail(FaultCode::LinkFailure);
            return self.outputs();
        }
        if inputs.link.quadrature_fault {
            self.fail(FaultCode::QuadratureFailure);
            return self.outputs();
        }
        if inputs.link.estop_pressed {
            if let Some(sample) = inputs.packet {
                if !self.recovery.active() {
                    self.engage_estop(sample);
                }
            }
            self.external_enable = false;
            return self.outputs();
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

        self.control_available = self.external_enable
            && self.startup_reset_complete
            && self.recovery_power_phase.is_none()
            && inputs.machine.ready_for_pendant_jog()
            && (self.phase.bounce() || (!any(inputs.raw_limits) && !any(inputs.safety_limits)));

        if self.pendant_mode_enabled && !self.phase.bounce() {
            if let Some(sample) = inputs.packet {
                match self.interpreter.process(sample) {
                    PendantDecision::Stop(_) => self.cancel_pendant_motion(),
                    PendantDecision::NoDetent => {}
                    PendantDecision::Jog(intent) => self.request_jog(intent, &inputs),
                    PendantDecision::Fault(_) => self.fail(FaultCode::InvalidPendantPacket),
                }
            }
        }
        self.control_ready = self.control_available
            && self.pendant_mode_enabled
            && !self.phase.bounce()
            && !any(inputs.raw_limits)
            && !any(inputs.safety_limits);
        self.outputs()
    }
}
