use crate::pendant::PendantSample;
use crate::startup::{
    ControllerWatchdogGuard, ControllerWatchdogPhase, HeartbeatGenerator, MesaStartupGuard,
    MesaStartupPhase,
};
use crate::supervisor::{
    FaultCode, LinkSnapshot, LinuxCncPendantSupervisor, MachineSnapshot, SupervisorInputs,
    SupervisorOutputs,
};
use crate::{Freshness, PENDANT_PACKET_TIMEOUT_NS, TASK_HEARTBEAT_TIMEOUT_NS};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RuntimeInputs {
    pub servo_thread_ready: bool,
    pub mesa_watchdog_has_bit: bool,
    pub mesa_io_error: bool,
    pub software_watchdog_ok: bool,
    pub ui_ready: bool,
    pub task_monitor_connected: bool,
    pub task_monitor_fault: bool,
    pub task_heartbeat: u32,
    pub pendant_coherent: bool,
    pub pendant_connected: bool,
    pub pendant_serial_fault: bool,
    pub pendant_quadrature_fault: bool,
    pub pendant_sample: PendantSample,
    pub machine: MachineSnapshot,
    pub counts_by_motor: [i32; 3],
    pub position_feedback_by_motor: [f64; 3],
    pub raw_limits: [bool; 3],
    pub safety_limits: [bool; 3],
    pub pendant_mode_enabled: bool,
    pub command_channel_ready: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RuntimeOutputs {
    pub heartbeat: bool,
    pub watchdog_enable: bool,
    pub mesa_watchdog_clear_requested: bool,
    pub position_known: bool,
    pub mesa_phase: MesaStartupPhase,
    pub controller_watchdog_phase: ControllerWatchdogPhase,
    pub supervisor: SupervisorOutputs,
    pub limit_reset: [bool; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RuntimeController {
    mesa_guard: MesaStartupGuard,
    watchdog_guard: ControllerWatchdogGuard,
    heartbeat: HeartbeatGenerator,
    task_freshness: Freshness,
    pendant_freshness: Freshness,
    supervisor: LinuxCncPendantSupervisor,
    last_pendant_sequence: Option<u32>,
    pendant_link_known: bool,
    pendant_link_valid: bool,
    pendant_connected: bool,
    pendant_serial_fault: bool,
    pendant_quadrature_fault: bool,
    pendant_estop_pressed: bool,
}

impl RuntimeController {
    pub const fn new() -> Self {
        Self {
            mesa_guard: MesaStartupGuard::new(),
            watchdog_guard: ControllerWatchdogGuard::new(),
            heartbeat: HeartbeatGenerator::new(),
            task_freshness: Freshness::new(),
            pendant_freshness: Freshness::new(),
            supervisor: LinuxCncPendantSupervisor::new(),
            last_pendant_sequence: None,
            pendant_link_known: false,
            pendant_link_valid: false,
            pendant_connected: false,
            pendant_serial_fault: true,
            pendant_quadrature_fault: false,
            pendant_estop_pressed: true,
        }
    }

    pub const fn supervisor(&self) -> &LinuxCncPendantSupervisor {
        &self.supervisor
    }

    pub fn fail(&mut self, fault: FaultCode) {
        self.supervisor.fail(fault);
    }

    pub fn update(&mut self, period_ns: u64, inputs: RuntimeInputs) -> RuntimeOutputs {
        let heartbeat = self.heartbeat.update(period_ns);
        if inputs.task_monitor_connected && !inputs.task_monitor_fault {
            self.task_freshness.update(inputs.task_heartbeat, period_ns);
        } else {
            self.task_freshness.elapse(period_ns);
        }

        if inputs.pendant_coherent {
            self.pendant_link_known = true;
            self.pendant_connected = inputs.pendant_connected;
            self.pendant_serial_fault = inputs.pendant_serial_fault;
            self.pendant_quadrature_fault = inputs.pendant_quadrature_fault;
            self.pendant_estop_pressed = inputs.pendant_sample.estop_pressed;
            self.pendant_link_valid = inputs.pendant_connected
                && !inputs.pendant_serial_fault
                && !inputs.pendant_quadrature_fault;
        }
        if inputs.pendant_coherent && self.pendant_link_valid {
            self.pendant_freshness
                .update(inputs.pendant_sample.sequence, period_ns);
        } else {
            self.pendant_freshness.elapse(period_ns);
        }

        self.mesa_guard.update(
            period_ns,
            inputs.servo_thread_ready,
            inputs.mesa_watchdog_has_bit,
            inputs.mesa_io_error,
        );
        if self.mesa_guard.faulted {
            self.supervisor.fail(FaultCode::MesaStartupFailure);
        }

        let task_valid = inputs.task_monitor_connected
            && !inputs.task_monitor_fault
            && self.task_freshness.is_fresh(TASK_HEARTBEAT_TIMEOUT_NS);
        let pendant_valid = self.pendant_link_known
            && self.pendant_link_valid
            && self.pendant_freshness.is_fresh(PENDANT_PACKET_TIMEOUT_NS);
        let prerequisites =
            self.mesa_guard.ready() && inputs.ui_ready && task_valid && pendant_valid;

        self.watchdog_guard
            .update(period_ns, inputs.software_watchdog_ok, prerequisites);
        if self.watchdog_guard.faulted {
            self.supervisor.fail(FaultCode::ControllerWatchdogFailure);
        }

        if self.watchdog_guard.runtime_committed {
            if !task_valid {
                self.supervisor.fail(FaultCode::TaskHeartbeatTimeout);
            } else if !pendant_valid {
                self.supervisor.fail(if self.pendant_quadrature_fault {
                    FaultCode::QuadratureFailure
                } else if self.pendant_serial_fault || !self.pendant_connected {
                    FaultCode::LinkFailure
                } else {
                    FaultCode::PacketTimeout
                });
            }
        }

        if self.mesa_guard.ready() && self.watchdog_guard.ready() {
            let new_packet = if inputs.pendant_coherent
                && self.pendant_link_valid
                && self.last_pendant_sequence != Some(inputs.pendant_sample.sequence)
            {
                self.last_pendant_sequence = Some(inputs.pendant_sample.sequence);
                Some(inputs.pendant_sample)
            } else {
                None
            };
            let link = LinkSnapshot {
                connected: self.pendant_connected,
                serial_fault: self.pendant_serial_fault,
                quadrature_fault: self.pendant_quadrature_fault,
                estop_pressed: self.pendant_estop_pressed,
            };
            self.supervisor.update(
                period_ns,
                SupervisorInputs {
                    link,
                    packet: new_packet,
                    machine: inputs.machine,
                    counts_by_motor: inputs.counts_by_motor,
                    position_feedback_by_motor: inputs.position_feedback_by_motor,
                    raw_limits: inputs.raw_limits,
                    safety_limits: inputs.safety_limits,
                    pendant_mode_enabled: inputs.pendant_mode_enabled,
                    command_channel_ready: inputs.command_channel_ready,
                },
            );
            if self.supervisor.startup_reset_complete() && !self.watchdog_guard.runtime_committed {
                if !self.watchdog_guard.commit_runtime() {
                    self.supervisor.fail(FaultCode::ControllerWatchdogFailure);
                }
            }
        }

        let supervisor = self.supervisor.outputs();
        let limit_reset = if self.mesa_guard.ready() {
            supervisor.limit_reset
        } else {
            self.mesa_guard.limit_reset
        };
        RuntimeOutputs {
            heartbeat,
            watchdog_enable: self.watchdog_guard.enable,
            mesa_watchdog_clear_requested: self.mesa_guard.watchdog_clear_requested,
            position_known: inputs.machine.all_homed() && task_valid,
            mesa_phase: self.mesa_guard.phase(),
            controller_watchdog_phase: self.watchdog_guard.phase(),
            supervisor,
            limit_reset,
        }
    }
}

impl Default for RuntimeController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pendant::{AxisSelector, MultiplierSelector};

    fn sample(sequence: u32) -> PendantSample {
        PendantSample {
            sequence,
            quadrature_errors: 0,
            latest_detent: 0,
            axis: AxisSelector::X,
            multiplier: MultiplierSelector::X1,
            deadman_held: false,
            estop_pressed: false,
            selector_valid: true,
        }
    }

    fn inputs(sequence: u32, heartbeat: u32) -> RuntimeInputs {
        RuntimeInputs {
            servo_thread_ready: true,
            mesa_watchdog_has_bit: false,
            mesa_io_error: false,
            software_watchdog_ok: true,
            ui_ready: true,
            task_monitor_connected: true,
            task_monitor_fault: false,
            task_heartbeat: heartbeat,
            pendant_coherent: true,
            pendant_connected: true,
            pendant_serial_fault: false,
            pendant_quadrature_fault: false,
            pendant_sample: sample(sequence),
            machine: MachineSnapshot {
                machine_on: true,
                estopped: false,
                manual_mode: true,
                joint_mode: false,
                teleop_mode: true,
                interp_idle: true,
                homed: [true; 3],
                homing: [false; 3],
                axis_stopped: [true; 3],
            },
            counts_by_motor: [0; 3],
            position_feedback_by_motor: [0.0; 3],
            raw_limits: [false; 3],
            safety_limits: [false; 3],
            pendant_mode_enabled: true,
            command_channel_ready: true,
        }
    }

    fn reach_runtime(controller: &mut RuntimeController) -> (u32, u32) {
        let mut sequence = 1_u32;
        let mut heartbeat = 1_u32;
        for cycle in 0..500 {
            if cycle % 20 == 0 {
                sequence = sequence.wrapping_add(1);
            }
            heartbeat = heartbeat.wrapping_add(1);
            controller.update(1_000_000, inputs(sequence, heartbeat));
            if controller.watchdog_guard.runtime_committed {
                return (sequence, heartbeat);
            }
        }
        panic!("runtime did not commit within the bounded test setup");
    }

    #[test]
    fn stale_task_heartbeat_can_never_reach_runtime_commit() {
        let mut controller = RuntimeController::new();
        let mut sequence = 1_u32;
        for cycle in 0..600 {
            if cycle % 20 == 0 {
                sequence = sequence.wrapping_add(1);
            }
            let output = controller.update(1_000_000, inputs(sequence, 7));
            assert!(!controller.watchdog_guard.runtime_committed);
            assert!(!output.supervisor.external_enable);
        }
    }

    #[test]
    fn task_freeze_after_commit_fails_closed_at_100_ms() {
        let mut controller = RuntimeController::new();
        let (mut sequence, heartbeat) = reach_runtime(&mut controller);
        for cycle in 0..99 {
            if cycle % 20 == 0 {
                sequence = sequence.wrapping_add(1);
            }
            let output = controller.update(1_000_000, inputs(sequence, heartbeat));
            assert!(output.supervisor.fault.is_none());
        }
        sequence = sequence.wrapping_add(1);
        let output = controller.update(1_000_000, inputs(sequence, heartbeat));
        assert_eq!(
            output.supervisor.fault,
            Some(FaultCode::TaskHeartbeatTimeout)
        );
        assert!(!output.supervisor.external_enable);
    }

    #[test]
    fn incoherent_pendant_publication_is_never_processed() {
        let mut controller = RuntimeController::new();
        let (sequence, heartbeat) = reach_runtime(&mut controller);
        let mut invalid = inputs(sequence.wrapping_add(1), heartbeat.wrapping_add(1));
        invalid.pendant_coherent = false;
        invalid.pendant_sample.latest_detent = 1;
        let output = controller.update(1_000_000, invalid);
        assert!(output.supervisor.command.is_none());
        assert!(output.supervisor.fault.is_none());
        assert!(output.supervisor.external_enable);
    }
}
