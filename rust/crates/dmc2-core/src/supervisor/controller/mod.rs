mod jog;
mod limits;
mod recovery;
mod startup;
mod update;

use super::{CommandEvent, FaultCode, JogCommand, SupervisorOutputs};
#[cfg(test)]
use super::{LinkSnapshot, MachineSnapshot, SupervisorInputs};
use crate::pendant::{PendantInterpreter, PendantSample};
use crate::recovery::EstopRecoverySequence;
use crate::{Axis, JogIntent, MOTOR_PULSE_SCALE, PULSES_PER_MM};

pub const BOUNCE_RATE_PULSES_PER_SECOND: i32 = 300 * MOTOR_PULSE_SCALE;
pub const BOUNCE_SPEED_MM_PER_MINUTE: i32 = BOUNCE_RATE_PULSES_PER_SECOND * 60 / PULSES_PER_MM;
pub const GATE_SETTLE_NS: u64 = 50_000_000;
pub const MOTION_SETTLE_NS: u64 = 25_000_000;
pub const BOUNCE_TIMEOUT_NS: u64 = 2_000_000_000;
pub const LIMIT_RESET_NS: u64 = 10_000_000;
pub const LIMIT_RESET_VALIDATE_NS: u64 = 10_000_000;
pub const LIMIT_RESET_TIMEOUT_NS: u64 = 100_000_000;

// HostMot2 stepgen position feedback retains all 16 fractional accumulator
// bits, while its signed count pin is the arithmetic-shifted integer portion.
// A completed integer-pulse request is valid when the feedback is within half
// a generated pulse of the exact target. At or beyond half a pulse there is no
// unique nearest integer target, so the request fails closed.
pub const JOG_TARGET_TOLERANCE_PULSES: f64 = 0.5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum Phase {
    Idle,
    StoppingCancel,
    StoppingReplace,
    StoppingBounce,
    Bouncing,
    BounceResetAssert,
    BounceResetValidate,
    StartupGateSettle,
    StartupWaitReset,
    StartupWaitOn,
    StartupReadyGateSettle,
    StartupReadyWaitReset,
    StartupReadyWaitOn,
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
                | Self::BounceResetAssert
                | Self::BounceResetValidate
        ) || self.startup_bounce()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ActiveJog {
    intent: JogIntent,
    joint_jog: bool,
    start_count: i32,
    target_count: i32,
    target_position_pulses: f64,
}

impl ActiveJog {
    const fn toward_positive_limit(self) -> bool {
        self.intent.delta_pulses > 0
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
    startup_reset_complete: bool,
    phase: Phase,
    phase_elapsed_ns: u64,
    phase_total_ns: u64,
    active: Option<ActiveJog>,
    pending: Option<JogIntent>,
    collision_motor: Option<usize>,
    bounce_start_count: Option<i32>,
    motion_not_before_ns: u64,
    fault: Option<FaultCode>,
    recovery_power_phase: Option<RecoveryPowerPhase>,
    recovery_elapsed_ns: u64,
    external_enable: bool,
    pendant_mode_enabled: bool,
    control_available: bool,
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
            startup_reset_complete: false,
            phase: Phase::Idle,
            phase_elapsed_ns: 0,
            phase_total_ns: 0,
            active: None,
            pending: None,
            collision_motor: None,
            bounce_start_count: None,
            motion_not_before_ns: 0,
            fault: None,
            recovery_power_phase: None,
            recovery_elapsed_ns: 0,
            external_enable: false,
            pendant_mode_enabled: false,
            control_available: false,
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
        self.fault
    }

    pub const fn startup_reset_complete(&self) -> bool {
        self.startup_reset_complete
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

    pub fn fail(&mut self, fault: FaultCode) {
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
        self.control_available = false;
        self.control_ready = false;
        self.clear_state_requests();
        self.fault = Some(fault);
    }

    pub fn outputs(&self) -> SupervisorOutputs {
        let mut command_enable = [false; 3];
        let mut toward_limit = [false; 3];
        if let Some(active) = self.active {
            if self.phase == Phase::Idle
                || self.phase == Phase::Bouncing
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
            control_available: self.control_available,
            control_ready: self.control_ready,
            estop_reset_request: self.estop_reset_request,
            machine_on_request: self.machine_on_request,
            fault: self.fault,
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

#[cfg(test)]
mod tests;
