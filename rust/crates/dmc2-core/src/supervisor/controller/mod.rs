mod jog;
mod limits;
mod recovery;
mod startup;
mod update;

use super::{
    CommandEvent, FaultCode, FaultRecord, JogCommand, JogPath, SupervisorInputs, SupervisorOutputs,
};
#[cfg(test)]
use super::{LinkSnapshot, MachineSnapshot};
use crate::pendant::{PendantInterpreter, PendantSample};
use crate::recovery::EstopRecoverySequence;
use crate::{Axis, JogIntent, BOUNCE_TARGET_PULSES_PER_SECOND, PULSES_PER_MM};
use dmc2_diagnostics::diagnostic_catalog;

pub const BOUNCE_RATE_PULSES_PER_SECOND: i32 = BOUNCE_TARGET_PULSES_PER_SECOND;
pub const BOUNCE_SPEED_MM_PER_MINUTE: i32 = BOUNCE_RATE_PULSES_PER_SECOND * 60 / PULSES_PER_MM;
pub const GATE_SETTLE_NS: u64 = 50_000_000;
pub const MOTION_ACCEPT_TIMEOUT_NS: u64 = 100_000_000;
pub const JOG_TIMEOUT_NS: u64 = crate::CONFIGURED_JOG_TIMEOUT_NS;
pub const MOTION_STOP_TIMEOUT_NS: u64 = crate::CONFIGURED_MOTION_STOP_TIMEOUT_NS;
pub const BOUNCE_TIMEOUT_NS: u64 = crate::CONFIGURED_BOUNCE_TIMEOUT_NS;
pub const LIMIT_RESET_NS: u64 = 10_000_000;
pub const LIMIT_RESET_VALIDATE_NS: u64 = 10_000_000;
pub const LIMIT_RESET_TIMEOUT_NS: u64 = 100_000_000;

// Manual pendant jogging is positioning rather than cutting. A completed x1,
// x10, or x100 increment therefore accepts an error up to and including 20%
// of that increment's requested pulse distance.
pub const JOG_TARGET_TOLERANCE_RATIO: f64 = 0.20;

const fn manual_target_tolerance_pulses(requested_delta_pulses: i32) -> f64 {
    let requested = requested_delta_pulses as f64;
    let magnitude = if requested < 0.0 {
        -requested
    } else {
        requested
    };
    magnitude * JOG_TARGET_TOLERANCE_RATIO
}

fn manual_target_reached(target_error_pulses: f64, requested_delta_pulses: i32) -> bool {
    let tolerance = manual_target_tolerance_pulses(requested_delta_pulses);
    target_error_pulses.is_finite()
        && target_error_pulses >= -tolerance
        && target_error_pulses <= tolerance
}

diagnostic_catalog! {
    pub enum Phase {
    Idle = 0,
    "IDLE",
    "idle",
    "the pendant supervisor has no active stop, jog, bounce, or startup-power transition",
    "no phase-specific action is required; use the named readiness and fault diagnostics";
    StoppingCancel = 1,
    "STOPPING_CANCEL",
    "stopping-cancel",
    "the supervisor is waiting for motion to stop after a cancelled pendant increment",
    "wait for LinuxCNC's realtime wheel-jog-active acknowledgement to clear or inspect the named timeout fault";
    StoppingReplace = 2,
    "STOPPING_REPLACE",
    "stopping-replace",
    "the supervisor is stopping the active increment before issuing the latest replacement increment",
    "wait for LinuxCNC's realtime wheel-jog-active acknowledgement to clear; inspect retained motion evidence if it faults";
    StoppingBounce = 3,
    "STOPPING_BOUNCE",
    "stopping-bounce",
    "the supervisor is stopping the colliding move before the configured limit backoff",
    "wait for LinuxCNC's realtime jog-active and attributed wheel-jog-active outputs to clear before backoff begins";
    Bouncing = 4,
    "BOUNCING",
    "bouncing",
    "the supervisor is executing the configured move away from an attributed limit",
    "allow the bounded backoff to finish; inspect the named bounce fault if it cannot";
    BounceResetAssert = 5,
    "BOUNCE_RESET_ASSERT",
    "bounce-reset-assert",
    "the supervisor is asserting the attributed realtime limit-latch reset after backoff",
    "wait for the bounded latch-reset interval";
    BounceResetValidate = 6,
    "BOUNCE_RESET_VALIDATE",
    "bounce-reset-validate",
    "the supervisor is verifying that the attributed limit latch cleared after backoff",
    "inspect the raw and safety-limit evidence if validation faults";
    StartupGateSettle = 7,
    "STARTUP_GATE_SETTLE",
    "startup-gate-settle",
    "startup found an asserted limit and is settling the disabled external command gate before backoff",
    "wait for the fixed gate-settle interval; do not command another move";
    StartupWaitReset = 8,
    "STARTUP_WAIT_RESET",
    "startup-wait-reset",
    "startup limit recovery is waiting for LinuxCNC to acknowledge E-stop reset",
    "inspect the named LinuxCNC state diagnostic if reset acknowledgement does not arrive";
    StartupWaitOn = 9,
    "STARTUP_WAIT_ON",
    "startup-wait-on",
    "startup limit recovery is waiting for LinuxCNC to acknowledge machine power on",
    "inspect the named LinuxCNC state diagnostic if machine-on acknowledgement does not arrive";
    StartupReadyGateSettle = 10,
    "STARTUP_READY_GATE_SETTLE",
    "startup-ready-gate-settle",
    "startup prerequisites are healthy and the disabled command gate is settling before power recovery",
    "wait for the fixed gate-settle interval";
    StartupReadyWaitReset = 11,
    "STARTUP_READY_WAIT_RESET",
    "startup-ready-wait-reset",
    "startup is waiting for LinuxCNC to acknowledge its E-stop reset request",
    "inspect the named LinuxCNC state diagnostic if acknowledgement does not arrive";
    StartupReadyWaitOn = 12,
    "STARTUP_READY_WAIT_ON",
    "startup-ready-wait-on",
    "startup is waiting for LinuxCNC to acknowledge its machine-on request",
    "inspect the named LinuxCNC state diagnostic if acknowledgement does not arrive";
    BounceReleaseWait = 13,
    "BOUNCE_RELEASE_WAIT",
    "bounce-release-wait",
    "the exact automatic backoff completed but the attributed raw limit remains active",
    "use the pendant deadman and command only the attributed axis away from its limit";
    BounceReleaseJog = 14,
    "BOUNCE_RELEASE_JOG",
    "bounce-release-jog",
    "the operator commanded one finite pendant increment away from the still-active attributed limit",
    "hold the deadman until the requested away increment completes or release it to stop";
    BounceReleaseStopping = 15,
    "BOUNCE_RELEASE_STOPPING",
    "bounce-release-stopping",
    "the supervisor is stopping an operator-commanded away increment during limit release",
    "wait for LinuxCNC's realtime wheel-jog acknowledgement to clear";
    }
}

impl Phase {
    const fn startup_bounce(self) -> bool {
        matches!(
            self,
            Self::StartupGateSettle | Self::StartupWaitReset | Self::StartupWaitOn
        )
    }

    const fn startup_power(self) -> bool {
        matches!(
            self,
            Self::StartupReadyGateSettle | Self::StartupReadyWaitReset | Self::StartupReadyWaitOn
        )
    }

    const fn bounce(self) -> bool {
        matches!(
            self,
            Self::StoppingBounce
                | Self::Bouncing
                | Self::BounceReleaseWait
                | Self::BounceReleaseJog
                | Self::BounceReleaseStopping
                | Self::BounceResetAssert
                | Self::BounceResetValidate
        ) || self.startup_bounce()
    }

    const fn timed_bounce(self) -> bool {
        self.bounce() && !matches!(self, Self::BounceReleaseWait)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ActiveJog {
    intent: JogIntent,
    path: JogPath,
    start_count: i32,
    start_position_pulses: f64,
    target_count: i32,
    target_position_pulses: f64,
    command_elapsed_ns: u64,
    consumer_active_seen: bool,
    feedback_progress_seen: bool,
}

impl ActiveJog {
    const fn toward_positive_limit(self) -> bool {
        self.intent.delta_pulses > 0
    }

    fn observe_motion(&mut self, period_ns: u64, inputs: &SupervisorInputs) {
        self.command_elapsed_ns = self.command_elapsed_ns.saturating_add(period_ns);
        if inputs.motion.wheel_active(self.intent.axis, self.path) {
            self.consumer_active_seen = true;
        }
        let motor = self.intent.motor;
        let position = inputs.position_feedback_by_motor[motor] * PULSES_PER_MM as f64;
        if inputs.counts_by_motor[motor] != self.start_count
            || (position.is_finite() && (position - self.start_position_pulses).abs() >= 0.25)
        {
            self.feedback_progress_seen = true;
        }
    }

    fn restart_observation(
        &mut self,
        start_count: i32,
        start_position_pulses: f64,
        target_count: i32,
        target_position_pulses: f64,
    ) {
        self.start_count = start_count;
        self.start_position_pulses = start_position_pulses;
        self.target_count = target_count;
        self.target_position_pulses = target_position_pulses;
        self.command_elapsed_ns = 0;
        self.consumer_active_seen = false;
        self.feedback_progress_seen = false;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecoveryPowerPhase {
    GateSettle,
    WaitReset,
    WaitOn,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinuxCncPendantSupervisor {
    interpreter: PendantInterpreter,
    recovery: EstopRecoverySequence,
    link_established: bool,
    startup_limits_checked: bool,
    phase: Phase,
    phase_elapsed_ns: u64,
    phase_total_ns: u64,
    active: Option<ActiveJog>,
    pending: Option<JogIntent>,
    collision_motor: Option<usize>,
    bounce_start_count: Option<i32>,
    fault: Option<FaultRecord>,
    last_inputs: Option<SupervisorInputs>,
    recovery_power_phase: Option<RecoveryPowerPhase>,
    recovery_restore_machine_on: bool,
    recovery_elapsed_ns: u64,
    external_enable: bool,
    pendant_mode_enabled: bool,
    control_ready: bool,
    limit_reset: [bool; 3],
    homing_was_active: bool,
    homing_reset_elapsed_ns: u64,
    command: Option<CommandEvent>,
    estop_reset_request: bool,
    machine_on_request: bool,
}

impl LinuxCncPendantSupervisor {
    pub const fn new() -> Self {
        Self {
            interpreter: PendantInterpreter::new(),
            recovery: EstopRecoverySequence::new(),
            link_established: false,
            startup_limits_checked: false,
            phase: Phase::Idle,
            phase_elapsed_ns: 0,
            phase_total_ns: 0,
            active: None,
            pending: None,
            collision_motor: None,
            bounce_start_count: None,
            fault: None,
            last_inputs: None,
            recovery_power_phase: None,
            recovery_restore_machine_on: false,
            recovery_elapsed_ns: 0,
            external_enable: false,
            pendant_mode_enabled: false,
            control_ready: false,
            limit_reset: [false; 3],
            homing_was_active: false,
            homing_reset_elapsed_ns: 0,
            command: None,
            estop_reset_request: false,
            machine_on_request: false,
        }
    }

    pub const fn fault(&self) -> Option<FaultCode> {
        match self.fault {
            Some(record) => Some(record.code),
            None => None,
        }
    }

    pub const fn fault_record(&self) -> Option<FaultRecord> {
        self.fault
    }

    /// Clear one retained controller fault without restoring machine power or
    /// accepting a motion command.  The runtime owns the current-input safety
    /// checks that authorize this transition.
    pub fn clear_latched_fault(&mut self) -> bool {
        if self.fault.is_none() {
            return false;
        }

        self.active = None;
        self.pending = None;
        self.transition(Phase::Idle);
        self.collision_motor = None;
        self.bounce_start_count = None;
        self.recovery = EstopRecoverySequence::new();
        self.recovery_power_phase = None;
        self.recovery_restore_machine_on = false;
        self.recovery_elapsed_ns = 0;
        self.limit_reset = [false; 3];
        self.homing_was_active = false;
        self.homing_reset_elapsed_ns = 0;
        self.external_enable = false;
        self.control_ready = false;
        self.clear_state_requests();
        self.interpreter.reset();
        self.startup_limits_checked = false;
        self.fault = None;
        // Reassert the native realtime stop while the cleared controller is
        // still deliberately held disabled for this complete update cycle.
        self.command = Some(CommandEvent::JogStopImmediate);
        true
    }

    pub fn observe_inputs(&mut self, inputs: SupervisorInputs) {
        self.last_inputs = Some(inputs);
    }

    pub const fn startup_sequence_complete(&self) -> bool {
        self.startup_limits_checked && !self.phase.startup_power() && !self.phase.startup_bounce()
    }

    fn transition(&mut self, phase: Phase) {
        self.phase = phase;
        self.phase_elapsed_ns = 0;
        self.phase_total_ns = 0;
    }

    fn clear_state_requests(&mut self) {
        self.estop_reset_request = false;
        self.machine_on_request = false;
    }

    fn fault_record_from_snapshot(&self, fault: FaultCode) -> FaultRecord {
        let mut record = FaultRecord::empty(fault);
        record.evidence.supervisor_phase = self.phase.wire_code();
        match fault {
            FaultCode::BounceTimedOut => {
                record.evidence.elapsed_ns = Some(self.phase_total_ns);
                record.evidence.timeout_ns = Some(BOUNCE_TIMEOUT_NS);
            }
            FaultCode::JogCommandNotAccepted => {
                record.evidence.elapsed_ns = active_elapsed(self.active);
                record.evidence.timeout_ns = Some(MOTION_ACCEPT_TIMEOUT_NS);
            }
            FaultCode::JogTimedOut => {
                record.evidence.elapsed_ns = active_elapsed(self.active);
                record.evidence.timeout_ns = Some(JOG_TIMEOUT_NS);
            }
            FaultCode::MotionStopTimedOut => {
                record.evidence.elapsed_ns = Some(self.phase_elapsed_ns);
                record.evidence.timeout_ns = Some(MOTION_STOP_TIMEOUT_NS);
            }
            FaultCode::LimitLatchResetTimedOut => {
                record.evidence.elapsed_ns = Some(self.phase_total_ns);
                record.evidence.timeout_ns = Some(LIMIT_RESET_TIMEOUT_NS);
            }
            _ => {}
        }

        let active = self.active;
        let motor = active
            .map(|value| value.intent.motor)
            .or(self.collision_motor);
        record.evidence.motor = motor;
        record.evidence.axis = active
            .map(|value| value.intent.axis)
            .or_else(|| motor.map(axis_by_motor));
        if let Some(active) = active {
            record.evidence.start_count = Some(active.start_count);
            record.evidence.target_count = Some(active.target_count);
            record.evidence.target_position_pulses = Some(active.target_position_pulses);
            record.evidence.consumer_active_seen = active.consumer_active_seen;
            record.evidence.feedback_progress_seen = active.feedback_progress_seen;
        }
        if let Some(collision_motor) = self.collision_motor {
            record.evidence.expected_limit_mask = Some(1_u32 << collision_motor);
        }

        if let Some(inputs) = self.last_inputs {
            record.evidence.counts_by_motor = inputs.counts_by_motor;
            record.evidence.position_feedback_by_motor = inputs.position_feedback_by_motor;
            record.evidence.raw_limit_mask = bit_mask(inputs.raw_limits);
            record.evidence.safety_limit_mask = bit_mask(inputs.safety_limits);
            record.evidence.link_connected = inputs.link.connected;
            record.evidence.serial_fault = inputs.link.serial_fault;
            record.evidence.quadrature_fault = inputs.link.quadrature_fault;
            record.evidence.pendant_estop_pressed = inputs.link.estop_pressed;
            record.evidence.machine_on = inputs.machine.machine_on;
            record.evidence.machine_estopped = inputs.machine.estopped;
            record.evidence.manual_mode = inputs.machine.manual_mode;
            record.evidence.joint_mode = inputs.machine.joint_mode;
            record.evidence.teleop_mode = inputs.machine.teleop_mode;
            record.evidence.interp_idle = inputs.machine.interp_idle;
            record.evidence.homed_mask = bit_mask(inputs.machine.homed);
            record.evidence.homing_mask = bit_mask(inputs.machine.homing);
            record.evidence.stopped_mask = bit_mask(inputs.machine.axis_stopped);
            record.evidence.motion_command_ready = inputs.motion_command_ready;
            record.evidence.motion_enabled = inputs.motion.enabled;
            record.evidence.motion_teleop_mode = inputs.motion.teleop_mode;
            record.evidence.motion_coord_mode = inputs.motion.coord_mode;
            record.evidence.motion_in_position = inputs.motion.in_position;
            record.evidence.motion_jog_active = inputs.motion.jog_active;
            record.evidence.axis_wheel_jog_active_mask =
                bit_mask(inputs.motion.axis_wheel_jog_active);
            record.evidence.joint_wheel_jog_active_mask =
                bit_mask(inputs.motion.joint_wheel_jog_active);
            record.evidence.joint_in_position_mask = bit_mask(inputs.motion.joint_in_position);
            if let Some(motor) = motor.filter(|value| *value < 3) {
                let observed_count = inputs.counts_by_motor[motor];
                let observed_position =
                    inputs.position_feedback_by_motor[motor] * PULSES_PER_MM as f64;
                record.evidence.observed_count = Some(observed_count);
                record.evidence.observed_position_pulses = Some(observed_position);
                if let Some(target) = record.evidence.target_position_pulses {
                    record.evidence.position_error_pulses = Some(observed_position - target);
                }
            }
        }
        record
    }

    fn latch_fault(&mut self, record: FaultRecord) {
        if self.fault.is_some() {
            return;
        }
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
        self.fault = Some(record);
    }

    pub fn fail(&mut self, fault: FaultCode) {
        let record = self.fault_record_from_snapshot(fault);
        self.latch_fault(record);
    }

    pub fn fail_with_runtime_evidence(
        &mut self,
        fault: FaultCode,
        task_heartbeat_age_ns: u64,
        pendant_packet_age_ns: u64,
        mesa_phase: i32,
        controller_watchdog_phase: i32,
    ) {
        let mut record = self.fault_record_from_snapshot(fault);
        record.evidence.task_heartbeat_age_ns = Some(task_heartbeat_age_ns);
        record.evidence.pendant_packet_age_ns = Some(pendant_packet_age_ns);
        record.evidence.mesa_phase = Some(mesa_phase);
        record.evidence.controller_watchdog_phase = Some(controller_watchdog_phase);
        match fault {
            FaultCode::TaskHeartbeatTimeout => {
                record.evidence.elapsed_ns = Some(task_heartbeat_age_ns);
                record.evidence.timeout_ns = Some(crate::TASK_HEARTBEAT_TIMEOUT_NS);
            }
            FaultCode::PacketTimeout => {
                record.evidence.elapsed_ns = Some(pendant_packet_age_ns);
                record.evidence.timeout_ns = Some(crate::PENDANT_PACKET_TIMEOUT_NS);
            }
            _ => {}
        }
        self.latch_fault(record);
    }

    pub fn outputs(&self) -> SupervisorOutputs {
        let mut command_enable = [false; 3];
        let mut toward_limit = [false; 3];
        if let Some(active) = self.active {
            if self.phase == Phase::Idle
                || self.phase == Phase::Bouncing
                || self.phase == Phase::BounceReleaseJog
                || self.phase.startup_bounce()
            {
                command_enable[active.intent.motor] = true;
            }
            if self.phase == Phase::Idle {
                toward_limit[active.intent.motor] = active.toward_positive_limit();
            }
        }
        SupervisorOutputs {
            external_enable: self.external_enable,
            control_ready: self.control_ready,
            estop_reset_request: self.estop_reset_request,
            machine_on_request: self.machine_on_request,
            fault: self.fault.map(|record| record.code),
            fault_record: self.fault,
            recovery_active: self.recovery.active() || self.recovery_power_phase.is_some(),
            jog_active: self.active.is_some(),
            bounce_active: self.phase.bounce(),
            active_axis: self.active.map(|active| active.intent.axis),
            phase: self.phase,
            limit_reset: self.limit_reset,
            command_enable_by_motor: command_enable,
            toward_limit_by_motor: toward_limit,
            command: self.command,
        }
    }
}

impl Default for LinuxCncPendantSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

const fn any(values: [bool; 3]) -> bool {
    values[0] || values[1] || values[2]
}

const fn bit_mask(values: [bool; 3]) -> u32 {
    values[0] as u32 | ((values[1] as u32) << 1) | ((values[2] as u32) << 2)
}

const fn active_elapsed(active: Option<ActiveJog>) -> Option<u64> {
    match active {
        Some(value) => Some(value.command_elapsed_ns),
        None => None,
    }
}

const fn one_hot(index: usize) -> [bool; 3] {
    [index == 0, index == 1, index == 2]
}

const fn single_active(values: [bool; 3]) -> Option<usize> {
    match values {
        [true, false, false] => Some(0),
        [false, true, false] => Some(1),
        [false, false, true] => Some(2),
        _ => None,
    }
}

const fn axis_by_motor(motor: usize) -> Axis {
    match motor {
        0 => Axis::Y,
        1 => Axis::X,
        2 => Axis::Z,
        _ => Axis::X,
    }
}
