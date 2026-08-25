use crate::pendant::{PendantDecision, PendantInterpreter, PendantSample};
use crate::recovery::EstopRecoverySequence;
use crate::{Axis, JogIntent, BOUNCE_PULSES, MOTOR_PULSE_SCALE, PULSES_PER_MM};

pub const BOUNCE_RATE_PULSES_PER_SECOND: i32 = 300 * MOTOR_PULSE_SCALE;
pub const BOUNCE_SPEED_MM_PER_MINUTE: i32 = BOUNCE_RATE_PULSES_PER_SECOND * 60 / PULSES_PER_MM;
pub const GATE_SETTLE_NS: u64 = 50_000_000;
pub const MOTION_SETTLE_NS: u64 = 25_000_000;
pub const BOUNCE_TIMEOUT_NS: u64 = 2_000_000_000;
pub const LIMIT_RESET_NS: u64 = 10_000_000;
pub const LIMIT_RESET_VALIDATE_NS: u64 = 10_000_000;
pub const LIMIT_RESET_TIMEOUT_NS: u64 = 100_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkSnapshot {
    pub connected: bool,
    pub serial_fault: bool,
    pub quadrature_fault: bool,
    pub estop_pressed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MachineSnapshot {
    pub machine_on: bool,
    pub estopped: bool,
    pub manual_mode: bool,
    pub joint_mode: bool,
    pub teleop_mode: bool,
    pub interp_idle: bool,
    pub homed: [bool; 3],
    pub homing: [bool; 3],
    pub axis_stopped: [bool; 3],
}

impl MachineSnapshot {
    pub const fn all_homed(&self) -> bool {
        self.homed[0] && self.homed[1] && self.homed[2]
    }

    pub const fn any_homing(&self) -> bool {
        self.homing[0] || self.homing[1] || self.homing[2]
    }

    pub const fn ready_for_pendant_jog(&self) -> bool {
        let jog_mode_ready = if self.all_homed() {
            self.teleop_mode
        } else {
            self.joint_mode
        };
        self.machine_on
            && !self.estopped
            && self.manual_mode
            && jog_mode_ready
            && self.interp_idle
            && !self.any_homing()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JogCommand {
    pub axis: Axis,
    pub joint_jog: bool,
    pub signed_delta_pulses: f64,
    pub speed_mm_per_minute: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CommandEvent {
    JogIncrement(JogCommand),
    JogStop,
    JogStopImmediate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum FaultCode {
    AdapterFailure,
    LinkFailure,
    QuadratureFailure,
    PacketTimeout,
    TaskHeartbeatTimeout,
    InvalidPendantPacket,
    StartupLimitMismatch,
    StartupLimitDuringHoming,
    LimitDuringStartupReset,
    HomingDuringStartupReset,
    LimitDuringRecovery,
    UnexpectedLimit,
    BounceLostLimitAttribution,
    BounceFeedbackUnavailable,
    BounceFeedbackIncoherent,
    BounceCountMismatch,
    JogCountMismatch,
    BounceLimitStillActive,
    BounceTimedOut,
    LimitLatchResetTimedOut,
    MesaStartupFailure,
    ControllerWatchdogFailure,
    CommandSequencerFailure,
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ActiveJog {
    intent: JogIntent,
    joint_jog: bool,
    start_count: i32,
    target_count: i32,
}

impl ActiveJog {
    const fn toward_positive_limit(self) -> bool {
        self.intent.delta_pulses > 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SupervisorOutputs {
    pub external_enable: bool,
    pub control_available: bool,
    pub control_ready: bool,
    pub estop_reset_request: bool,
    pub machine_on_request: bool,
    pub fault: Option<FaultCode>,
    pub recovery_active: bool,
    pub jog_active: bool,
    pub bounce_active: bool,
    pub active_axis: Option<Axis>,
    pub phase: Phase,
    pub limit_reset: [bool; 3],
    pub command_enable_by_motor: [bool; 3],
    pub toward_limit_by_motor: [bool; 3],
    pub command: Option<CommandEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SupervisorInputs {
    pub link: LinkSnapshot,
    pub packet: Option<PendantSample>,
    pub machine: MachineSnapshot,
    pub counts_by_motor: [i32; 3],
    pub position_feedback_by_motor: [f64; 3],
    pub raw_limits: [bool; 3],
    pub safety_limits: [bool; 3],
    pub pendant_mode_enabled: bool,
    pub command_channel_ready: bool,
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

    fn cancel_pendant_motion(&mut self) {
        self.pending = None;
        if self.active.is_none() || self.phase.bounce() {
            return;
        }
        self.command = Some(CommandEvent::JogStop);
        self.transition(Phase::StoppingCancel);
    }

    fn engage_estop(&mut self, sample: PendantSample) {
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

    fn advance_recovery(&mut self, period_ns: u64, inputs: &SupervisorInputs) {
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

    fn emit_jog(&mut self, command: JogCommand, ready: bool) -> bool {
        if !ready || self.command.is_some() {
            return false;
        }
        self.command = Some(CommandEvent::JogIncrement(command));
        true
    }

    fn start_jog(&mut self, intent: JogIntent, inputs: &SupervisorInputs) -> bool {
        if !inputs.machine.ready_for_pendant_jog()
            || any(inputs.raw_limits)
            || any(inputs.safety_limits)
        {
            return false;
        }
        let joint_jog = !inputs.machine.all_homed();
        let command = JogCommand {
            axis: intent.axis,
            joint_jog,
            signed_delta_pulses: intent.delta_pulses as f64,
            speed_mm_per_minute: intent.speed_mm_per_minute,
        };
        if !self.emit_jog(command, inputs.command_channel_ready) {
            self.pending = Some(intent);
            return false;
        }
        let start_count = inputs.counts_by_motor[intent.motor];
        self.active = Some(ActiveJog {
            intent,
            joint_jog,
            start_count,
            target_count: start_count.wrapping_add(intent.delta_pulses),
        });
        self.pending = None;
        self.transition(Phase::Idle);
        self.motion_not_before_ns = MOTION_SETTLE_NS;
        true
    }

    fn request_jog(&mut self, intent: JogIntent, inputs: &SupervisorInputs) {
        if self.phase.bounce() {
            return;
        }
        let Some(active) = self.active else {
            let _ = self.start_jog(intent, inputs);
            return;
        };

        let same_motion = self.phase == Phase::Idle
            && active.intent.motor == intent.motor
            && active.intent.axis == intent.axis
            && active.joint_jog == !inputs.machine.all_homed()
            && active.toward_positive_limit() == (intent.delta_pulses > 0);
        if same_motion && inputs.command_channel_ready {
            let current_count = inputs.counts_by_motor[intent.motor];
            let replacement_target = current_count.wrapping_add(intent.delta_pulses);
            let extension = replacement_target.wrapping_sub(active.target_count);
            if extension == 0 {
                if let Some(value) = self.active.as_mut() {
                    value.intent = intent;
                }
                self.pending = None;
                return;
            }
            if (extension > 0) == (intent.delta_pulses > 0) {
                let command = JogCommand {
                    axis: intent.axis,
                    joint_jog: active.joint_jog,
                    signed_delta_pulses: extension as f64,
                    speed_mm_per_minute: intent.speed_mm_per_minute,
                };
                if self.emit_jog(command, true) {
                    if let Some(value) = self.active.as_mut() {
                        value.intent = intent;
                        value.target_count = replacement_target;
                    }
                    self.pending = None;
                    self.motion_not_before_ns = MOTION_SETTLE_NS;
                    return;
                }
            }
        }

        self.pending = Some(intent);
        if same_motion {
            return;
        }
        if self.phase != Phase::StoppingReplace {
            self.command = Some(CommandEvent::JogStop);
            self.transition(Phase::StoppingReplace);
        }
    }

    fn begin_bounce(&mut self, motor: usize) {
        if self.active.is_none() {
            self.fail(FaultCode::UnexpectedLimit);
            return;
        }
        self.command = Some(CommandEvent::JogStopImmediate);
        self.pending = None;
        self.transition(Phase::StoppingBounce);
        self.collision_motor = Some(motor);
        self.bounce_start_count = None;
    }

    fn begin_startup_power(&mut self) {
        self.clear_state_requests();
        self.transition(Phase::StartupReadyGateSettle);
        self.external_enable = true;
    }

    fn finish_startup_power(&mut self) {
        self.clear_state_requests();
        self.transition(Phase::Idle);
        self.startup_reset_complete = true;
        self.interpreter.reset();
    }

    fn advance_startup_power(&mut self, inputs: &SupervisorInputs) {
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

    fn begin_startup_bounce(&mut self, motor: usize, inputs: &SupervisorInputs) {
        self.clear_state_requests();
        let axis = axis_by_motor(motor);
        let start_count = inputs.counts_by_motor[motor];
        self.active = Some(ActiveJog {
            intent: JogIntent {
                axis,
                motor,
                delta_pulses: -BOUNCE_PULSES,
                speed_mm_per_minute: BOUNCE_SPEED_MM_PER_MINUTE,
            },
            joint_jog: !inputs.machine.all_homed(),
            start_count,
            target_count: start_count.wrapping_sub(BOUNCE_PULSES),
        });
        self.pending = None;
        self.collision_motor = Some(motor);
        self.bounce_start_count = None;
        self.transition(Phase::StartupGateSettle);
        self.external_enable = true;
    }

    fn check_startup_limits(&mut self, inputs: &SupervisorInputs) {
        if self.startup_limits_checked {
            return;
        }
        self.startup_limits_checked = true;
        if !any(inputs.raw_limits) && !any(inputs.safety_limits) {
            if inputs.machine.estopped || !inputs.machine.machine_on {
                self.begin_startup_power();
            } else {
                self.clear_state_requests();
                self.startup_reset_complete = true;
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
        self.begin_startup_bounce(raw_motor.unwrap_or(0), inputs);
    }

    fn advance_startup_bounce(&mut self, inputs: &SupervisorInputs) {
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
        if !inputs.machine.ready_for_pendant_jog() {
            return;
        }
        self.clear_state_requests();
        self.startup_reset_complete = true;
        self.start_bounce_move(inputs);
    }

    fn check_limits(&mut self, inputs: &SupervisorInputs) {
        if !any(inputs.safety_limits) || inputs.machine.any_homing() || self.homing_was_active {
            return;
        }
        let active_motor = single_active(inputs.safety_limits);
        if self.phase.bounce() {
            if active_motor != self.collision_motor {
                self.fail(FaultCode::BounceLostLimitAttribution);
            }
            return;
        }
        if let (Some(active), Some(motor)) = (self.active, active_motor) {
            if active.toward_positive_limit() && active.intent.motor == motor {
                self.begin_bounce(motor);
                return;
            }
        }
        self.fail(FaultCode::UnexpectedLimit);
    }

    fn start_bounce_move(&mut self, inputs: &SupervisorInputs) {
        let Some(motor) = self.collision_motor else {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        };
        if inputs.safety_limits != one_hot(motor) {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        }
        let Some(active) = self.active else {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        };
        let start_count = inputs.counts_by_motor[motor];
        let position_pulses = inputs.position_feedback_by_motor[motor] * PULSES_PER_MM as f64;
        let mut fractional_phase = position_pulses - start_count as f64;
        if !fractional_phase.is_finite()
            || fractional_phase < -0.000_001
            || fractional_phase > 1.000_001
        {
            self.fail(FaultCode::BounceFeedbackIncoherent);
            return;
        }
        if fractional_phase < 0.0 {
            fractional_phase = 0.0;
        }
        let below_one = f64::from_bits(1.0_f64.to_bits() - 1);
        if fractional_phase >= 1.0 {
            fractional_phase = below_one;
        }
        let distance_pulses = BOUNCE_PULSES as f64 - 0.5 + fractional_phase;
        let command = JogCommand {
            axis: axis_by_motor(motor),
            joint_jog: active.joint_jog,
            signed_delta_pulses: -distance_pulses,
            speed_mm_per_minute: BOUNCE_SPEED_MM_PER_MINUTE,
        };
        if !self.emit_jog(command, inputs.command_channel_ready) {
            return;
        }
        self.bounce_start_count = Some(start_count);
        self.transition(Phase::Bouncing);
        self.motion_not_before_ns = MOTION_SETTLE_NS;
    }

    fn finish_bounce(&mut self, inputs: &SupervisorInputs) {
        let (Some(motor), Some(start_count)) = (self.collision_motor, self.bounce_start_count)
        else {
            self.fail(FaultCode::BounceLostLimitAttribution);
            return;
        };
        let actual_delta = inputs.counts_by_motor[motor].wrapping_sub(start_count);
        if actual_delta != -BOUNCE_PULSES {
            self.fail(FaultCode::BounceCountMismatch);
            return;
        }
        if inputs.raw_limits[motor] {
            self.fail(FaultCode::BounceLimitStillActive);
            return;
        }
        self.limit_reset[motor] = true;
        self.transition(Phase::BounceResetAssert);
    }

    fn advance_motion(&mut self, period_ns: u64, inputs: &SupervisorInputs) {
        if self.motion_not_before_ns > 0 {
            self.motion_not_before_ns = self.motion_not_before_ns.saturating_sub(period_ns);
        }
        if self.phase.bounce() {
            self.phase_total_ns = self.phase_total_ns.saturating_add(period_ns);
            if self.phase_total_ns > BOUNCE_TIMEOUT_NS {
                self.fail(FaultCode::BounceTimedOut);
                return;
            }
        }

        match self.phase {
            Phase::BounceResetAssert => {
                if self.phase_elapsed_ns < LIMIT_RESET_NS {
                    return;
                }
                self.limit_reset = [false; 3];
                self.transition(Phase::BounceResetValidate);
                return;
            }
            Phase::BounceResetValidate => {
                if self.phase_total_ns > LIMIT_RESET_TIMEOUT_NS {
                    self.fail(FaultCode::LimitLatchResetTimedOut);
                    return;
                }
                if self.phase_elapsed_ns < LIMIT_RESET_VALIDATE_NS || any(inputs.safety_limits) {
                    return;
                }
                self.active = None;
                self.pending = None;
                self.transition(Phase::Idle);
                self.collision_motor = None;
                self.bounce_start_count = None;
                self.interpreter.reset();
                return;
            }
            _ => {}
        }

        let Some(active) = self.active else {
            if self.phase == Phase::Idle && self.pending.is_some() && inputs.command_channel_ready {
                if let Some(intent) = self.pending.take() {
                    let _ = self.start_jog(intent, inputs);
                }
            }
            return;
        };
        if !inputs.command_channel_ready
            || !inputs.machine.axis_stopped[active.intent.axis.index()]
            || self.motion_not_before_ns > 0
        {
            return;
        }
        match self.phase {
            Phase::StoppingBounce => self.start_bounce_move(inputs),
            Phase::Bouncing => self.finish_bounce(inputs),
            Phase::StoppingReplace => {
                self.active = None;
                self.transition(Phase::Idle);
                if let Some(intent) = self.pending.take() {
                    let _ = self.start_jog(intent, inputs);
                }
            }
            Phase::Idle => {
                if inputs.counts_by_motor[active.intent.motor] != active.target_count {
                    self.fail(FaultCode::JogCountMismatch);
                    return;
                }
                self.active = None;
                self.transition(Phase::Idle);
                if let Some(intent) = self.pending.take() {
                    let _ = self.start_jog(intent, inputs);
                }
            }
            Phase::StoppingCancel => {
                self.active = None;
                self.pending = None;
                self.transition(Phase::Idle);
            }
            _ => {}
        }
    }

    fn manage_homing_latches(&mut self, period_ns: u64, inputs: &SupervisorInputs) {
        if inputs.machine.any_homing() {
            self.homing_was_active = true;
            self.homing_reset_elapsed_ns = 0;
            self.cancel_pendant_motion();
            return;
        }
        if !self.homing_was_active || any(inputs.raw_limits) {
            return;
        }
        if !any(self.limit_reset) && any(inputs.safety_limits) {
            self.limit_reset = inputs.safety_limits;
            self.homing_reset_elapsed_ns = 0;
            return;
        }
        if any(self.limit_reset) {
            self.homing_reset_elapsed_ns = self.homing_reset_elapsed_ns.saturating_add(period_ns);
            if self.homing_reset_elapsed_ns >= LIMIT_RESET_NS {
                self.limit_reset = [false; 3];
                self.homing_reset_elapsed_ns = 0;
            }
            return;
        }
        if !any(inputs.safety_limits) {
            self.homing_was_active = false;
            self.homing_reset_elapsed_ns = 0;
        }
    }

    fn drain_pending(&mut self, inputs: &SupervisorInputs) {
        if self.phase != Phase::Idle || !inputs.command_channel_ready || self.pending.is_none() {
            return;
        }
        let intent = self.pending.take();
        if let Some(intent) = intent {
            if self.active.is_some() {
                self.request_jog(intent, inputs);
            } else {
                let _ = self.start_jog(intent, inputs);
            }
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
            if let Some(sample) = inputs.packet {
                let update = self.recovery.process(sample);
                if update.unlock_requested {
                    self.begin_recovery_unlock(&inputs);
                }
            }
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
mod tests {
    use super::*;
    use crate::pendant::{AxisSelector, MultiplierSelector};

    fn sample(sequence: u32, detent: i32, deadman: bool) -> PendantSample {
        PendantSample {
            sequence,
            quadrature_errors: 0,
            latest_detent: detent,
            axis: AxisSelector::X,
            multiplier: MultiplierSelector::X1,
            deadman_held: deadman,
            estop_pressed: false,
            selector_valid: true,
        }
    }

    fn ready_machine() -> MachineSnapshot {
        MachineSnapshot {
            machine_on: true,
            estopped: false,
            manual_mode: true,
            joint_mode: false,
            teleop_mode: true,
            interp_idle: true,
            homed: [true; 3],
            homing: [false; 3],
            axis_stopped: [true; 3],
        }
    }

    fn inputs(packet: Option<PendantSample>) -> SupervisorInputs {
        SupervisorInputs {
            link: LinkSnapshot {
                connected: true,
                serial_fault: false,
                quadrature_fault: false,
                estop_pressed: packet.is_some_and(|value| value.estop_pressed),
            },
            packet,
            machine: ready_machine(),
            counts_by_motor: [1_000, 2_000, 3_000],
            position_feedback_by_motor: [1.0, 2.0, 3.0],
            raw_limits: [false; 3],
            safety_limits: [false; 3],
            pendant_mode_enabled: true,
            command_channel_ready: true,
        }
    }

    fn arm(supervisor: &mut LinuxCncPendantSupervisor) {
        supervisor.update(1_000_000, inputs(Some(sample(1, 0, false))));
        supervisor.update(20_000_000, inputs(Some(sample(2, 0, true))));
    }

    #[test]
    fn clockwise_x_is_one_exact_negative_ten_pulse_linuxcnc_request() {
        let mut supervisor = LinuxCncPendantSupervisor::new();
        arm(&mut supervisor);
        let output = supervisor.update(20_000_000, inputs(Some(sample(3, 1, true))));
        assert_eq!(
            output.command,
            Some(CommandEvent::JogIncrement(JogCommand {
                axis: Axis::X,
                joint_jog: false,
                signed_delta_pulses: -10.0,
                speed_mm_per_minute: 300,
            }))
        );
        assert_eq!(output.active_axis, Some(Axis::X));
        assert!(output.command_enable_by_motor[1]);
        assert!(!output.toward_limit_by_motor[1]);
    }

    #[test]
    fn deadman_release_requests_linuxcnc_jog_stop() {
        let mut supervisor = LinuxCncPendantSupervisor::new();
        arm(&mut supervisor);
        supervisor.update(20_000_000, inputs(Some(sample(3, 1, true))));
        let mut released = inputs(Some(sample(4, 0, false)));
        released.machine.axis_stopped[0] = false;
        let output = supervisor.update(1_000_000, released);
        assert_eq!(output.command, Some(CommandEvent::JogStop));
        assert_eq!(output.phase, Phase::StoppingCancel);
    }

    #[test]
    fn completed_increment_requires_the_exact_generated_pulse_count() {
        let mut supervisor = LinuxCncPendantSupervisor::new();
        arm(&mut supervisor);
        supervisor.update(20_000_000, inputs(Some(sample(3, 1, true))));

        let mut completed = inputs(Some(sample(4, 0, true)));
        completed.counts_by_motor[1] = 1_990;
        completed.position_feedback_by_motor[1] = 1.990;
        let output = supervisor.update(25_000_000, completed);

        assert!(output.fault.is_none());
        assert!(!output.jog_active);
    }

    #[test]
    fn missed_increment_fails_closed_instead_of_reporting_success() {
        let mut supervisor = LinuxCncPendantSupervisor::new();
        arm(&mut supervisor);
        supervisor.update(20_000_000, inputs(Some(sample(3, 1, true))));

        let output = supervisor.update(25_000_000, inputs(Some(sample(4, 0, true))));

        assert_eq!(output.fault, Some(FaultCode::JogCountMismatch));
        assert_eq!(output.command, Some(CommandEvent::JogStopImmediate));
        assert!(!output.external_enable);
    }

    #[test]
    fn reversal_storage_is_one_replaceable_slot() {
        let mut supervisor = LinuxCncPendantSupervisor::new();
        arm(&mut supervisor);
        supervisor.update(20_000_000, inputs(Some(sample(3, 1, true))));

        let mut moving = inputs(Some(sample(4, -1, true)));
        moving.machine.axis_stopped[0] = false;
        let first = supervisor.update(1_000_000, moving);
        assert_eq!(first.command, Some(CommandEvent::JogStop));
        assert_eq!(first.phase, Phase::StoppingReplace);

        let mut replaced = inputs(Some(sample(5, 1, true)));
        replaced.machine.axis_stopped[0] = false;
        let second = supervisor.update(1_000_000, replaced);
        assert_eq!(second.command, None);
        assert_eq!(second.phase, Phase::StoppingReplace);

        let completed = supervisor.update(25_000_000, inputs(Some(sample(6, 0, true))));
        assert_eq!(
            completed.command,
            Some(CommandEvent::JogIncrement(JogCommand {
                axis: Axis::X,
                joint_jog: false,
                signed_delta_pulses: -10.0,
                speed_mm_per_minute: 300,
            }))
        );
    }

    #[test]
    fn matching_positive_limit_runs_exact_negative_250_pulse_bounce() {
        let mut supervisor = LinuxCncPendantSupervisor::new();
        arm(&mut supervisor);
        supervisor.update(20_000_000, inputs(Some(sample(3, -1, true))));

        let mut collision = inputs(Some(sample(4, 0, true)));
        collision.machine.axis_stopped[0] = false;
        collision.raw_limits = [false, true, false];
        collision.safety_limits = [false, true, false];
        let stopped = supervisor.update(1_000_000, collision);
        assert_eq!(stopped.command, Some(CommandEvent::JogStopImmediate));
        assert_eq!(stopped.phase, Phase::StoppingBounce);

        let mut bounce = inputs(Some(sample(5, 0, true)));
        bounce.raw_limits = [false, true, false];
        bounce.safety_limits = [false, true, false];
        bounce.counts_by_motor[1] = 2_005;
        bounce.position_feedback_by_motor[1] = 2.005;
        let bounce_output = supervisor.update(25_000_000, bounce);
        assert_eq!(
            bounce_output.command,
            Some(CommandEvent::JogIncrement(JogCommand {
                axis: Axis::X,
                joint_jog: false,
                signed_delta_pulses: -249.5,
                speed_mm_per_minute: 90,
            }))
        );
        assert_eq!(bounce_output.phase, Phase::Bouncing);

        let mut finished = bounce;
        finished.raw_limits = [false; 3];
        finished.counts_by_motor[1] = 1_755;
        finished.position_feedback_by_motor[1] = 1.755;
        let reset = supervisor.update(25_000_000, finished);
        assert_eq!(reset.limit_reset, [false, true, false]);
        assert_eq!(reset.phase, Phase::BounceResetAssert);

        let asserted = supervisor.update(10_000_000, finished);
        assert_eq!(asserted.limit_reset, [false; 3]);
        assert_eq!(asserted.phase, Phase::BounceResetValidate);
        let mut cleared = finished;
        cleared.safety_limits = [false; 3];
        let complete = supervisor.update(10_000_000, cleared);
        assert_eq!(complete.phase, Phase::Idle);
        assert!(!complete.bounce_active);
        assert!(complete.fault.is_none());
    }

    #[test]
    fn non_exact_bounce_count_fails_closed() {
        let mut supervisor = LinuxCncPendantSupervisor::new();
        arm(&mut supervisor);
        supervisor.update(20_000_000, inputs(Some(sample(3, -1, true))));
        let mut collision = inputs(Some(sample(4, 0, true)));
        collision.raw_limits = [false, true, false];
        collision.safety_limits = [false, true, false];
        collision.machine.axis_stopped[0] = false;
        supervisor.update(1_000_000, collision);
        collision.machine.axis_stopped[0] = true;
        collision.counts_by_motor[1] = 2_005;
        collision.position_feedback_by_motor[1] = 2.005;
        supervisor.update(25_000_000, collision);
        collision.raw_limits = [false; 3];
        collision.counts_by_motor[1] = 1_756;
        let output = supervisor.update(25_000_000, collision);
        assert_eq!(output.fault, Some(FaultCode::BounceCountMismatch));
        assert!(!output.external_enable);
    }

    #[test]
    fn wrong_limit_during_jog_faults_without_bounce() {
        let mut supervisor = LinuxCncPendantSupervisor::new();
        arm(&mut supervisor);
        supervisor.update(20_000_000, inputs(Some(sample(3, -1, true))));
        let mut wrong = inputs(Some(sample(4, 0, true)));
        wrong.machine.axis_stopped[0] = false;
        wrong.safety_limits = [true, false, false];
        let output = supervisor.update(1_000_000, wrong);
        assert_eq!(output.fault, Some(FaultCode::UnexpectedLimit));
        assert_eq!(output.command, Some(CommandEvent::JogStopImmediate));
        assert!(!output.external_enable);
    }

    #[test]
    fn explicit_task_timeout_fault_cannot_leave_stale_status_accepted() {
        let mut supervisor = LinuxCncPendantSupervisor::new();
        arm(&mut supervisor);
        supervisor.fail(FaultCode::TaskHeartbeatTimeout);
        let output = supervisor.update(1_000_000, inputs(None));
        assert_eq!(output.fault, Some(FaultCode::TaskHeartbeatTimeout));
        assert!(!output.external_enable);
        assert!(!output.control_ready);
    }
}
