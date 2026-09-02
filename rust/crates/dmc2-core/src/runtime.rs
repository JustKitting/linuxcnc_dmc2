use crate::pendant::PendantSample;
use crate::startup::{
    ControllerWatchdogGuard, ControllerWatchdogPhase, HeartbeatGenerator, MesaStartupGuard,
    MesaStartupPhase,
};
use crate::supervisor::{
    FaultCode, LinkSnapshot, LinuxCncPendantSupervisor, MachineSnapshot, MotionSnapshot,
    SupervisorInputs, SupervisorOutputs,
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
    pub motion: MotionSnapshot,
    pub counts_by_motor: [i32; 3],
    pub position_feedback_by_motor: [f64; 3],
    pub raw_limits: [bool; 3],
    pub safety_limits: [bool; 3],
    pub pendant_mode_enabled: bool,
    pub motion_command_ready: bool,
    /// LinuxCNC's single canonical E-stop reset request
    /// (`iocontrol.0.user-request-enable`).
    pub linuxcnc_estop_reset_request: bool,
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
    pub fault_reset_allowed: bool,
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
    linuxcnc_estop_reset_request_held: bool,
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
            linuxcnc_estop_reset_request_held: false,
        }
    }

    pub const fn supervisor(&self) -> &LinuxCncPendantSupervisor {
        &self.supervisor
    }

    pub fn fail(&mut self, fault: FaultCode) {
        self.fail_runtime(fault);
    }

    fn fail_runtime(&mut self, fault: FaultCode) {
        let task_age = self.task_freshness.age_ns();
        let pendant_age = self.pendant_freshness.age_ns();
        let mesa_phase = self.mesa_guard.phase().wire_code();
        let watchdog_phase = self.watchdog_guard.phase().wire_code();
        self.supervisor.fail_with_runtime_evidence(
            fault,
            task_age,
            pendant_age,
            mesa_phase,
            watchdog_phase,
        );
    }

    fn fault_reset_allowed(&self, inputs: &RuntimeInputs, pendant_valid: bool) -> bool {
        self.supervisor.fault().is_some()
            && inputs.servo_thread_ready
            && !inputs.mesa_watchdog_has_bit
            && !inputs.mesa_io_error
            && pendant_valid
            && !self.pendant_estop_pressed
    }

    fn clear_fault_state(&mut self) -> bool {
        if self.supervisor.fault().is_none() {
            return false;
        }
        if self.mesa_guard.faulted {
            self.mesa_guard = MesaStartupGuard::new();
        }
        if self.watchdog_guard.faulted {
            self.watchdog_guard = ControllerWatchdogGuard::new();
        }
        self.supervisor.clear_latched_fault()
    }

    pub fn update(&mut self, period_ns: u64, inputs: RuntimeInputs) -> RuntimeOutputs {
        let linuxcnc_estop_reset_rising =
            inputs.linuxcnc_estop_reset_request && !self.linuxcnc_estop_reset_request_held;
        self.linuxcnc_estop_reset_request_held = inputs.linuxcnc_estop_reset_request;
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

        self.supervisor.observe_inputs(SupervisorInputs {
            link: LinkSnapshot {
                connected: self.pendant_connected,
                serial_fault: self.pendant_serial_fault,
                quadrature_fault: self.pendant_quadrature_fault,
                estop_pressed: self.pendant_estop_pressed,
            },
            packet: inputs.pendant_coherent.then_some(inputs.pendant_sample),
            machine: inputs.machine,
            motion: inputs.motion,
            counts_by_motor: inputs.counts_by_motor,
            position_feedback_by_motor: inputs.position_feedback_by_motor,
            raw_limits: inputs.raw_limits,
            safety_limits: inputs.safety_limits,
            pendant_mode_enabled: inputs.pendant_mode_enabled,
            motion_command_ready: inputs.motion_command_ready,
            linuxcnc_estop_reset_rising,
        });

        self.mesa_guard.update(
            period_ns,
            inputs.servo_thread_ready,
            inputs.mesa_watchdog_has_bit,
            inputs.mesa_io_error,
        );
        if self.mesa_guard.faulted {
            self.fail_runtime(FaultCode::MesaStartupFailure);
        }

        let task_valid = inputs.task_monitor_connected
            && !inputs.task_monitor_fault
            && self.task_freshness.is_fresh(TASK_HEARTBEAT_TIMEOUT_NS);
        let pendant_valid = self.pendant_link_known
            && self.pendant_link_valid
            && self.pendant_freshness.is_fresh(PENDANT_PACKET_TIMEOUT_NS);
        let fault_reset_allowed = self.fault_reset_allowed(&inputs, pendant_valid);
        if linuxcnc_estop_reset_rising && fault_reset_allowed && self.clear_fault_state() {
            self.supervisor.begin_linuxcnc_estop_reset();
            let supervisor = self.supervisor.outputs();
            return RuntimeOutputs {
                heartbeat,
                watchdog_enable: self.watchdog_guard.enable,
                mesa_watchdog_clear_requested: self.mesa_guard.watchdog_clear_requested,
                position_known: inputs.machine.all_homed() && task_valid,
                mesa_phase: self.mesa_guard.phase(),
                controller_watchdog_phase: self.watchdog_guard.phase(),
                supervisor,
                limit_reset: [false; 3],
                fault_reset_allowed: false,
            };
        }
        let prerequisites =
            self.mesa_guard.ready() && inputs.ui_ready && task_valid && pendant_valid;

        self.watchdog_guard
            .update(period_ns, inputs.software_watchdog_ok, prerequisites);
        if self.watchdog_guard.faulted {
            self.fail_runtime(FaultCode::ControllerWatchdogFailure);
        }

        let estop_active = self.pendant_estop_pressed
            || inputs.machine.estopped
            || self.supervisor.outputs().recovery_active;
        if self.watchdog_guard.runtime_committed && !estop_active {
            if !pendant_valid {
                let fault = if self.pendant_quadrature_fault {
                    FaultCode::QuadratureFailure
                } else if self.pendant_serial_fault || !self.pendant_connected {
                    FaultCode::LinkFailure
                } else {
                    FaultCode::PacketTimeout
                };
                self.fail_runtime(fault);
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
            // LinuxCNC task status is a userspace control prerequisite, not an
            // E-stop source. If that snapshot becomes stale, atomically disarm
            // Pendant Mode so no new jog can be accepted. The independent
            // realtime heartbeat and physical pendant E-stop continue to own
            // the global E-stop gate.
            self.supervisor.update(
                period_ns,
                SupervisorInputs {
                    link,
                    packet: new_packet,
                    machine: inputs.machine,
                    motion: inputs.motion,
                    counts_by_motor: inputs.counts_by_motor,
                    position_feedback_by_motor: inputs.position_feedback_by_motor,
                    raw_limits: inputs.raw_limits,
                    safety_limits: inputs.safety_limits,
                    pendant_mode_enabled: inputs.pendant_mode_enabled && task_valid,
                    motion_command_ready: inputs.motion_command_ready,
                    linuxcnc_estop_reset_rising,
                },
            );
            if self.supervisor.startup_reset_complete()
                && !self.watchdog_guard.runtime_committed
                && !self.watchdog_guard.commit_runtime()
            {
                self.fail_runtime(FaultCode::ControllerWatchdogFailure);
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
            fault_reset_allowed: self.fault_reset_allowed(&inputs, pendant_valid),
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

    fn inputs(sequence: u32, task_heartbeat: u32) -> RuntimeInputs {
        RuntimeInputs {
            servo_thread_ready: true,
            mesa_watchdog_has_bit: false,
            mesa_io_error: false,
            software_watchdog_ok: true,
            ui_ready: true,
            task_monitor_connected: true,
            task_monitor_fault: false,
            task_heartbeat,
            pendant_coherent: true,
            pendant_connected: true,
            pendant_serial_fault: false,
            pendant_quadrature_fault: false,
            pendant_sample: PendantSample {
                sequence,
                quadrature_errors: 0,
                latest_detent: 0,
                axis: AxisSelector::X,
                multiplier: MultiplierSelector::X1,
                deadman_held: false,
                estop_pressed: false,
                selector_valid: true,
            },
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
            motion: MotionSnapshot {
                enabled: true,
                teleop_mode: true,
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
            linuxcnc_estop_reset_request: false,
        }
    }

    fn reach_committed_runtime(controller: &mut RuntimeController) -> (u32, u32) {
        let mut sequence = 1;
        let mut heartbeat = 1;
        for cycle in 0..1_000 {
            if cycle % 20 == 0 {
                sequence += 1;
            }
            heartbeat += 1;
            controller.update(1_000_000, inputs(sequence, heartbeat));
            if controller.watchdog_guard.runtime_committed {
                return (sequence, heartbeat);
            }
        }
        panic!("runtime did not commit during the bounded setup");
    }

    #[test]
    fn stale_task_status_disarms_pendant_without_asserting_machine_estop() {
        let mut controller = RuntimeController::new();
        let (mut sequence, frozen_heartbeat) = reach_committed_runtime(&mut controller);
        for cycle in 0..150 {
            if cycle % 20 == 0 {
                sequence += 1;
            }
            let mut stale = inputs(sequence, frozen_heartbeat);
            stale.pendant_mode_enabled = true;
            stale.machine.manual_mode = false;
            stale.machine.teleop_mode = false;
            stale.machine.interp_idle = false;
            stale.motion.teleop_mode = false;
            stale.motion.coord_mode = true;
            let output = controller.update(1_000_000, stale);
            assert!(output.supervisor.fault.is_none());
            assert!(output.supervisor.external_enable);
            if cycle >= 99 {
                assert!(!output.supervisor.control_ready);
            }
        }
    }

    #[test]
    fn physical_pendant_estop_still_drops_the_global_gate() {
        let mut controller = RuntimeController::new();
        let (sequence, heartbeat) = reach_committed_runtime(&mut controller);
        let mut pressed = inputs(sequence + 1, heartbeat + 1);
        pressed.pendant_sample.estop_pressed = true;
        let output = controller.update(1_000_000, pressed);
        assert!(!output.supervisor.external_enable);
        assert!(output.supervisor.recovery_active);
    }

    #[test]
    fn unaccepted_manual_jog_returns_idle_without_fault_or_estop() {
        let mut controller = RuntimeController::new();
        let (mut sequence, mut heartbeat) = reach_committed_runtime(&mut controller);

        for latest_detent in [0, 0, 1] {
            sequence += 1;
            heartbeat += 1;
            let mut jog = inputs(sequence, heartbeat);
            jog.pendant_mode_enabled = true;
            jog.pendant_sample.axis = AxisSelector::Y;
            jog.pendant_sample.multiplier = MultiplierSelector::X100;
            jog.pendant_sample.deadman_held = true;
            jog.pendant_sample.latest_detent = latest_detent;
            controller.update(1_000_000, jog);
        }

        let mut output = controller.supervisor().outputs();
        for _ in 0..101 {
            sequence += 1;
            heartbeat += 1;
            let mut rejected = inputs(sequence, heartbeat);
            rejected.pendant_mode_enabled = true;
            rejected.pendant_sample.axis = AxisSelector::Y;
            rejected.pendant_sample.multiplier = MultiplierSelector::X100;
            rejected.pendant_sample.deadman_held = true;
            output = controller.update(1_000_000, rejected).supervisor;
        }

        assert!(output.fault.is_none());
        assert!(output.external_enable);
        assert!(output.control_ready);
        assert!(!output.jog_active);
        assert_eq!(output.phase, crate::supervisor::Phase::Idle);
    }
}
