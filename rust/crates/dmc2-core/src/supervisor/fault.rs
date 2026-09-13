use dmc2_diagnostics::{diagnostic_catalog, RecoveryClass, RecoveryClassified};

use crate::Axis;

diagnostic_catalog! {
    pub enum FaultCode {
    AdapterFailure = 1,
    "ADAPTER_FAILURE",
    "adapter-failure",
    "the LinuxCNC adapter rejected or could not publish a controller command",
    "inspect the retained command and adapter state before retrying";
    LinkFailure = 2,
    "LINK_FAILURE",
    "link-failure",
    "the pendant serial link is disconnected or reporting a transport fault",
    "restore the pendant link and verify a fresh coherent packet";
    QuadratureFailure = 3,
    "QUADRATURE_FAILURE",
    "quadrature-failure",
    "the pendant wheel decoder reported a quadrature transition error",
    "restore the wheel signal, release the E-stop and deadman, then use Clear Fault; a fresh packet with an unchanged decoder counter must acknowledge the reset before Pendant Mode can resume";
    PacketTimeout = 4,
    "PACKET_TIMEOUT",
    "packet-timeout",
    "no fresh coherent pendant packet arrived before the runtime deadline",
    "inspect packet age, sequence, serial transport, and Nano operation";
    TaskHeartbeatTimeout = 5,
    "TASK_HEARTBEAT_TIMEOUT",
    "task-heartbeat-timeout",
    "the LinuxCNC task-status heartbeat stopped changing before its deadline",
    "inspect the retained heartbeat age and task-monitor connection";
    InvalidPendantPacket = 6,
    "INVALID_PENDANT_PACKET",
    "invalid-pendant-packet",
    "a coherent packet contained a selector or detent state the controller cannot execute",
    "inspect the retained selector, detent, and packet sequence";
    StartupLimitMismatch = 7,
    "STARTUP_LIMIT_MISMATCH",
    "startup-limit-mismatch",
    "startup raw and safety-limit vectors do not identify one matching motor",
    "compare the retained raw and latched limit masks before resetting";
    StartupLimitDuringHoming = 8,
    "STARTUP_LIMIT_DURING_HOMING",
    "startup-limit-during-homing",
    "startup found an asserted limit while LinuxCNC already reported homing active",
    "end the conflicting homing state and inspect the retained limit masks";
    LimitDuringStartupReset = 9,
    "LIMIT_DURING_STARTUP_RESET",
    "limit-during-startup-reset",
    "a raw or latched limit asserted during the startup power-reset sequence",
    "inspect the retained limit masks and startup phase";
    HomingDuringStartupReset = 10,
    "HOMING_DURING_STARTUP_RESET",
    "homing-during-startup-reset",
    "LinuxCNC began homing during the startup power-reset sequence",
    "end the conflicting homing operation and inspect the startup phase";
    LimitDuringRecovery = 11,
    "LIMIT_DURING_RECOVERY",
    "limit-during-recovery",
    "a raw or latched limit asserted during pendant E-stop recovery",
    "inspect the retained limit masks before attempting recovery again";
    UnexpectedLimit = 12,
    "UNEXPECTED_LIMIT",
    "unexpected-limit",
    "a limit asserted without one attributable active move toward that same limit",
    "inspect active axis, motor, direction, and retained limit masks";
    BounceLostLimitAttribution = 13,
    "BOUNCE_LOST_LIMIT_ATTRIBUTION",
    "bounce-lost-limit-attribution",
    "the backoff sequence lost a unique match between its motor and asserted limit",
    "inspect the retained motor and expected/observed limit masks";
    BounceFeedbackUnavailable = 14,
    "BOUNCE_FEEDBACK_UNAVAILABLE",
    "bounce-feedback-unavailable",
    "backoff position feedback was non-finite and could not be evaluated",
    "inspect the retained motor count and position-feedback value";
    BounceFeedbackIncoherent = 15,
    "BOUNCE_FEEDBACK_INCOHERENT",
    "bounce-feedback-incoherent",
    "backoff count and fractional position feedback disagree",
    "compare the retained count and position feedback for the attributed motor";
    BounceCountMismatch = 16,
    "BOUNCE_COUNT_MISMATCH",
    "bounce-count-mismatch",
    "the completed limit backoff ended more than 20 percent of its requested distance from the target",
    "compare retained start, target, observed position, and error against the 20 percent manual-move tolerance";
    JogCountMismatch = 17,
    "JOG_COUNT_MISMATCH",
    "jog-count-mismatch",
    "a completed pendant increment ended more than 20 percent of its requested distance from the target",
    "compare retained axis, motor, target, observed position, and error against the 20 percent manual-jog tolerance";
    BounceLimitStillActive = 18,
    "BOUNCE_LIMIT_STILL_ACTIVE",
    "bounce-limit-still-active",
    "the raw limit remained asserted after the toleranced backoff target was reached",
    "inspect the retained motor, raw limit mask, and physical switch state";
    BounceTimedOut = 19,
    "BOUNCE_TIMED_OUT",
    "bounce-timed-out",
    "the limit backoff sequence exceeded its bounded completion time",
    "inspect retained elapsed time, phase, command state, and motor feedback";
    LimitLatchResetTimedOut = 20,
    "LIMIT_LATCH_RESET_TIMED_OUT",
    "limit-latch-reset-timed-out",
    "the realtime safety-limit latch did not clear before its reset deadline",
    "inspect retained expected and observed safety-limit masks";
    MesaStartupFailure = 21,
    "MESA_STARTUP_FAILURE",
    "mesa-startup-failure",
    "the Mesa startup guard detected an I/O error or could not clear its watchdog state",
    "use Clear Fault with the machine stopped to acknowledge the retained Mesa I/O/watchdog error; if communication does not recover, restore the Mesa connection and retry Clear Fault";
    ControllerWatchdogFailure = 22,
    "CONTROLLER_WATCHDOG_FAILURE",
    "controller-watchdog-failure",
    "the controller heartbeat watchdog failed its startup or runtime contract",
    "inspect retained watchdog phase, software-watchdog state, and prerequisites";
    MotionCommandEncodingFailure = 23,
    "MOTION_COMMAND_ENCODING_FAILURE",
    "motion-command-encoding-failure",
    "the native realtime motion publisher could not encode a controller command it had advertised ready to accept",
    "inspect retained command phase, active axis, requested increment, and publisher state";
    JogFeedbackUnavailable = 24,
    "JOG_FEEDBACK_UNAVAILABLE",
    "jog-feedback-unavailable",
    "pendant-jog position feedback was non-finite and could not be evaluated",
    "inspect the retained motor count and position-feedback value";
    JogFeedbackIncoherent = 25,
    "JOG_FEEDBACK_INCOHERENT",
    "jog-feedback-incoherent",
    "pendant-jog count and fractional position feedback disagree",
    "compare the retained count and position feedback for the active motor";
    MotionPathUnavailable = 26,
    "MOTION_PATH_UNAVAILABLE",
    "motion-path-unavailable",
    "LinuxCNC realtime motion state did not permit the selected axis or joint wheel-jog path",
    "inspect motion enabled, teleop, coordinated-mode, homing, and selected path evidence";
    JogCommandNotAccepted = 27,
    "JOG_COMMAND_NOT_ACCEPTED",
    "jog-command-not-accepted",
    "LinuxCNC did not assert the selected realtime wheel-jog-active output after the native count delta",
    "inspect selected path mode, enable, limits, feed hold, homing state, and returned motion evidence";
    JogTimedOut = 28,
    "JOG_TIMED_OUT",
    "jog-timed-out",
    "LinuxCNC accepted the finite wheel jog but did not reach its feedback target before the bounded deadline",
    "inspect retained consumer-active, feedback-progress, target, observed position, and motion state";
    MotionStopTimedOut = 29,
    "MOTION_STOP_TIMED_OUT",
    "motion-stop-timed-out",
    "LinuxCNC realtime jog-active state did not clear after a bounded stop request",
    "inspect retained wheel-jog-active masks, global jog-active state, and selected motion path";
    MotionInvalidPulsesPerMillimeter = 30,
    "MOTION_INVALID_PULSES_PER_MILLIMETER",
    "motion-invalid-pulses-per-millimeter",
    "the native motion publisher received a non-positive machine pulse scale",
    "inspect the compiled PULSES_PER_MM value and live INI pulse-scale contract";
    MotionInvalidServoPeriod = 31,
    "MOTION_INVALID_SERVO_PERIOD",
    "motion-invalid-servo-period",
    "the native motion publisher received a zero servo period",
    "inspect the realtime callback period and live EMCMOT SERVO_PERIOD";
    MotionInvalidTargetRate = 32,
    "MOTION_INVALID_TARGET_RATE",
    "motion-invalid-target-rate",
    "the native motion publisher received a non-positive or non-finite target-issuance rate",
    "inspect the selected multiplier and compiled target-rate policy";
    MotionNonFiniteDistance = 33,
    "MOTION_NON_FINITE_DISTANCE",
    "motion-non-finite-distance",
    "the native motion publisher received a non-finite finite-jog distance",
    "inspect the retained active command and distance calculation";
    MotionZeroDistance = 34,
    "MOTION_ZERO_DISTANCE",
    "motion-zero-distance",
    "the native motion publisher received a zero-distance finite-jog command",
    "inspect the selected multiplier and command replacement calculation";
    MotionCountRangeExceeded = 35,
    "MOTION_COUNT_RANGE_EXCEEDED",
    "motion-count-range-exceeded",
    "the finite target cannot be issued at the requested rate without exceeding the signed HAL count range",
    "inspect the requested distance, target rate, servo period, and count-range calculation";
    }
}

impl RecoveryClassified for FaultCode {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::LinkFailure
            | Self::QuadratureFailure
            | Self::PacketTimeout
            | Self::InvalidPendantPacket => RecoveryClass::RestorePendant,
            Self::StartupLimitMismatch
            | Self::StartupLimitDuringHoming
            | Self::LimitDuringStartupReset
            | Self::HomingDuringStartupReset
            | Self::LimitDuringRecovery
            | Self::UnexpectedLimit
            | Self::BounceLostLimitAttribution
            | Self::BounceFeedbackUnavailable
            | Self::BounceFeedbackIncoherent
            | Self::BounceCountMismatch
            | Self::BounceLimitStillActive
            | Self::BounceTimedOut
            | Self::LimitLatchResetTimedOut => RecoveryClass::ReleaseLimit,
            Self::AdapterFailure
            | Self::MesaStartupFailure
            | Self::ControllerWatchdogFailure
            | Self::JogCountMismatch
            | Self::JogFeedbackUnavailable
            | Self::JogFeedbackIncoherent
            | Self::MotionPathUnavailable
            | Self::JogCommandNotAccepted
            | Self::JogTimedOut
            | Self::MotionStopTimedOut
            | Self::MotionZeroDistance => RecoveryClass::ClearController,
            Self::TaskHeartbeatTimeout
            | Self::MotionCommandEncodingFailure
            | Self::MotionInvalidPulsesPerMillimeter
            | Self::MotionInvalidServoPeriod
            | Self::MotionInvalidTargetRate
            | Self::MotionNonFiniteDistance
            | Self::MotionCountRangeExceeded => RecoveryClass::RelaunchApplication,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaultEvidence {
    pub supervisor_phase: i32,
    pub axis: Option<Axis>,
    pub motor: Option<usize>,
    pub start_count: Option<i32>,
    pub target_count: Option<i32>,
    pub observed_count: Option<i32>,
    pub target_position_pulses: Option<f64>,
    pub observed_position_pulses: Option<f64>,
    pub position_error_pulses: Option<f64>,
    pub counts_by_motor: [i32; 3],
    pub position_feedback_by_motor: [f64; 3],
    pub raw_limit_mask: u32,
    pub safety_limit_mask: u32,
    pub expected_limit_mask: Option<u32>,
    pub elapsed_ns: Option<u64>,
    pub timeout_ns: Option<u64>,
    pub link_connected: bool,
    pub serial_fault: bool,
    pub quadrature_fault: bool,
    pub pendant_estop_pressed: bool,
    pub machine_on: bool,
    pub machine_estopped: bool,
    pub manual_mode: bool,
    pub joint_mode: bool,
    pub teleop_mode: bool,
    pub interp_idle: bool,
    pub homed_mask: u32,
    pub homing_mask: u32,
    pub stopped_mask: u32,
    pub motion_command_ready: bool,
    pub motion_enabled: bool,
    pub motion_teleop_mode: bool,
    pub motion_coord_mode: bool,
    pub motion_in_position: bool,
    pub motion_jog_active: bool,
    pub axis_wheel_jog_active_mask: u32,
    pub joint_wheel_jog_active_mask: u32,
    pub joint_in_position_mask: u32,
    pub consumer_active_seen: bool,
    pub feedback_progress_seen: bool,
    pub task_heartbeat_age_ns: Option<u64>,
    pub pendant_packet_age_ns: Option<u64>,
    pub mesa_phase: Option<i32>,
    pub controller_watchdog_phase: Option<i32>,
}

impl FaultEvidence {
    pub const fn empty() -> Self {
        Self {
            supervisor_phase: 0,
            axis: None,
            motor: None,
            start_count: None,
            target_count: None,
            observed_count: None,
            target_position_pulses: None,
            observed_position_pulses: None,
            position_error_pulses: None,
            counts_by_motor: [0; 3],
            position_feedback_by_motor: [0.0; 3],
            raw_limit_mask: 0,
            safety_limit_mask: 0,
            expected_limit_mask: None,
            elapsed_ns: None,
            timeout_ns: None,
            link_connected: false,
            serial_fault: true,
            quadrature_fault: false,
            pendant_estop_pressed: true,
            machine_on: false,
            machine_estopped: true,
            manual_mode: false,
            joint_mode: false,
            teleop_mode: false,
            interp_idle: false,
            homed_mask: 0,
            homing_mask: 0,
            stopped_mask: 0,
            motion_command_ready: false,
            motion_enabled: false,
            motion_teleop_mode: false,
            motion_coord_mode: false,
            motion_in_position: false,
            motion_jog_active: false,
            axis_wheel_jog_active_mask: 0,
            joint_wheel_jog_active_mask: 0,
            joint_in_position_mask: 0,
            consumer_active_seen: false,
            feedback_progress_seen: false,
            task_heartbeat_age_ns: None,
            pendant_packet_age_ns: None,
            mesa_phase: None,
            controller_watchdog_phase: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaultRecord {
    pub code: FaultCode,
    pub evidence: FaultEvidence,
}

impl FaultRecord {
    pub const fn empty(code: FaultCode) -> Self {
        Self {
            code,
            evidence: FaultEvidence::empty(),
        }
    }
}
