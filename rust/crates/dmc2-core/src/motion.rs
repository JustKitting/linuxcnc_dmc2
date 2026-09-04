//! Realtime LinuxCNC wheel-jog command transport.
//!
//! LinuxCNC 2.9.10 consumes `axis.*.jog-*` and `joint.*.jog-*` in the
//! motion servo thread.  This channel deliberately does not generate HALUI
//! edges: HALUI is a userspace/NML boundary and cannot acknowledge a command
//! on the realtime schedule.

use crate::supervisor::{CommandEvent, JogCommand, JogPath};
use dmc2_diagnostics::diagnostic_catalog;

diagnostic_catalog! {
    pub enum MotionCommandPhase: i32 {
        Idle = 0 => (
            "IDLE",
            "idle",
            "the native realtime motion command channel can accept one jog increment",
            "no motion-command transport action is required"
        ),
        JogCount = 1 => (
            "JOG_COUNT",
            "jog-count",
            "one axis or joint jog-count delta is enabled for LinuxCNC's current servo cycle",
            "inspect LinuxCNC wheel-jog activity and returned motion feedback"
        ),
        Rebase = 2 => (
            "REBASE",
            "rebase",
            "jog counts are returning to zero while disabled so the next command cannot overflow",
            "wait one servo cycle before publishing another finite increment"
        ),
        Stop = 3 => (
            "STOP",
            "stop",
            "the native realtime controlled-stop input is asserted for this servo cycle",
            "wait for LinuxCNC's realtime jog-active output to clear"
        ),
        StopImmediate = 4 => (
            "STOP_IMMEDIATE",
            "stop-immediate",
            "the native realtime immediate-stop input is asserted for this servo cycle",
            "wait for LinuxCNC's realtime jog-active output to clear"
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionCommandError {
    ChannelRebasing,
    InvalidPulsesPerMillimeter,
    InvalidServoPeriod,
    InvalidTargetRate,
    NonFiniteDistance,
    ZeroDistance,
    CountRangeExceeded,
}

impl MotionCommandError {
    pub const fn fault_code(self) -> crate::supervisor::FaultCode {
        use crate::supervisor::FaultCode;
        match self {
            Self::ChannelRebasing => FaultCode::MotionCommandEncodingFailure,
            Self::InvalidPulsesPerMillimeter => FaultCode::MotionInvalidPulsesPerMillimeter,
            Self::InvalidServoPeriod => FaultCode::MotionInvalidServoPeriod,
            Self::InvalidTargetRate => FaultCode::MotionInvalidTargetRate,
            Self::NonFiniteDistance => FaultCode::MotionNonFiniteDistance,
            Self::ZeroDistance => FaultCode::MotionZeroDistance,
            Self::CountRangeExceeded => FaultCode::MotionCountRangeExceeded,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeMotionOutputs {
    pub axis_jog_counts: [i32; 3],
    pub joint_jog_counts: [i32; 3],
    pub axis_jog_scale: [f64; 3],
    pub joint_jog_scale: [f64; 3],
    pub axis_jog_enable: [bool; 3],
    pub joint_jog_enable: [bool; 3],
    pub axis_jog_vel_mode: [bool; 3],
    pub joint_jog_vel_mode: [bool; 3],
    pub jog_stop: bool,
    pub jog_stop_immediate: bool,
    pub phase: MotionCommandPhase,
}

impl NativeMotionOutputs {
    const fn new() -> Self {
        Self {
            axis_jog_counts: [0; 3],
            joint_jog_counts: [0; 3],
            axis_jog_scale: [0.0; 3],
            joint_jog_scale: [0.0; 3],
            axis_jog_enable: [false; 3],
            joint_jog_enable: [false; 3],
            // LinuxCNC position mode executes the complete requested distance.
            axis_jog_vel_mode: [false; 3],
            joint_jog_vel_mode: [false; 3],
            jog_stop: false,
            jog_stop_immediate: false,
            phase: MotionCommandPhase::Idle,
        }
    }

    fn clear_transient(&mut self) {
        self.axis_jog_scale = [0.0; 3];
        self.joint_jog_scale = [0.0; 3];
        self.axis_jog_enable = [false; 3];
        self.joint_jog_enable = [false; 3];
        self.axis_jog_vel_mode = [false; 3];
        self.joint_jog_vel_mode = [false; 3];
        self.jog_stop = false;
        self.jog_stop_immediate = false;
        self.phase = MotionCommandPhase::Idle;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeMotionCommandChannel {
    outputs: NativeMotionOutputs,
    active: Option<ScheduledIncrement>,
    rebase_pending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ScheduledIncrement {
    axis: usize,
    path: JogPath,
    direction: i32,
    remaining_pulses: f64,
    pulses_per_mm: f64,
    target_pulses_per_second: f64,
}

impl NativeMotionCommandChannel {
    pub const fn new() -> Self {
        Self {
            outputs: NativeMotionOutputs::new(),
            active: None,
            rebase_pending: false,
        }
    }

    /// True only when an increment can be represented in the current cycle.
    /// Stop commands are accepted independently of this value.
    pub const fn ready(&self) -> bool {
        self.active.is_none()
            && !self.rebase_pending
            && matches!(self.outputs.phase, MotionCommandPhase::Idle)
    }

    /// Begin one new servo-cycle publication.
    ///
    /// Each finite command advances a cumulative count at its configured
    /// target-issuance rate.  Every count delta carries that servo cycle's
    /// exact distance in `jog-scale`.  A disabled cycle returns the count to
    /// zero after the final chunk.  LinuxCNC updates its private old-count
    /// baseline even while disabled, so later commands start from zero.
    pub fn advance(&mut self, period_ns: u64) -> Result<(), MotionCommandError> {
        self.outputs.clear_transient();
        if self.active.is_some() {
            return self.publish_next_chunk(period_ns);
        }
        if self.rebase_pending {
            self.outputs.axis_jog_counts = [0; 3];
            self.outputs.joint_jog_counts = [0; 3];
            self.outputs.phase = MotionCommandPhase::Rebase;
            self.rebase_pending = false;
        }
        Ok(())
    }

    pub fn accept(
        &mut self,
        event: CommandEvent,
        pulses_per_mm: i32,
        period_ns: u64,
    ) -> Result<(), MotionCommandError> {
        match event {
            CommandEvent::JogIncrement(command) => {
                self.accept_increment(command, pulses_per_mm, period_ns)
            }
            CommandEvent::JogStop => {
                self.cancel_increment();
                self.outputs.jog_stop = true;
                self.outputs.phase = MotionCommandPhase::Stop;
                Ok(())
            }
            CommandEvent::JogStopImmediate => {
                self.force_stop_immediate();
                Ok(())
            }
        }
    }

    fn accept_increment(
        &mut self,
        command: JogCommand,
        pulses_per_mm: i32,
        period_ns: u64,
    ) -> Result<(), MotionCommandError> {
        if !self.ready() {
            return Err(MotionCommandError::ChannelRebasing);
        }
        if pulses_per_mm <= 0 {
            return Err(MotionCommandError::InvalidPulsesPerMillimeter);
        }
        if period_ns == 0 {
            return Err(MotionCommandError::InvalidServoPeriod);
        }
        if command.target_rate_mm_per_minute <= 0 {
            return Err(MotionCommandError::InvalidTargetRate);
        }
        if !command.signed_delta_pulses.is_finite() {
            return Err(MotionCommandError::NonFiniteDistance);
        }
        if command.signed_delta_pulses == 0.0 {
            return Err(MotionCommandError::ZeroDistance);
        }

        let direction = if command.signed_delta_pulses > 0.0 {
            1
        } else {
            -1
        };
        let pulses_per_mm = pulses_per_mm as f64;
        let target_pulses_per_second =
            command.target_rate_mm_per_minute as f64 * pulses_per_mm / 60.0;
        let pulses_per_cycle = target_pulses_per_second * period_ns as f64 / 1_000_000_000.0;
        if !target_pulses_per_second.is_finite()
            || !pulses_per_cycle.is_finite()
            || pulses_per_cycle <= 0.0
        {
            return Err(MotionCommandError::InvalidTargetRate);
        }
        let required_cycles = command.signed_delta_pulses.abs() / pulses_per_cycle;
        if !required_cycles.is_finite() || required_cycles > i32::MAX as f64 {
            return Err(MotionCommandError::CountRangeExceeded);
        }

        self.active = Some(ScheduledIncrement {
            axis: command.axis.index(),
            path: command.path,
            direction,
            remaining_pulses: command.signed_delta_pulses.abs(),
            pulses_per_mm,
            target_pulses_per_second,
        });
        self.publish_next_chunk(period_ns)
    }

    fn publish_next_chunk(&mut self, period_ns: u64) -> Result<(), MotionCommandError> {
        if period_ns == 0 {
            return Err(MotionCommandError::InvalidServoPeriod);
        }
        let Some(mut active) = self.active else {
            return Ok(());
        };
        let budget = active.target_pulses_per_second * period_ns as f64 / 1_000_000_000.0;
        if !budget.is_finite() || budget <= 0.0 {
            return Err(MotionCommandError::InvalidTargetRate);
        }
        let chunk = active.remaining_pulses.min(budget);
        let scale = chunk / active.pulses_per_mm;
        let counts = match active.path {
            JogPath::AxisTeleop => &mut self.outputs.axis_jog_counts[active.axis],
            JogPath::JointFree => &mut self.outputs.joint_jog_counts[active.axis],
        };
        *counts = counts
            .checked_add(active.direction)
            .ok_or(MotionCommandError::CountRangeExceeded)?;
        match active.path {
            JogPath::AxisTeleop => {
                self.outputs.axis_jog_scale[active.axis] = scale;
                self.outputs.axis_jog_enable[active.axis] = true;
            }
            JogPath::JointFree => {
                self.outputs.joint_jog_scale[active.axis] = scale;
                self.outputs.joint_jog_enable[active.axis] = true;
            }
        }
        self.outputs.phase = MotionCommandPhase::JogCount;

        if chunk >= active.remaining_pulses {
            self.active = None;
            self.rebase_pending = true;
        } else {
            active.remaining_pulses -= chunk;
            self.active = Some(active);
        }
        Ok(())
    }

    pub fn force_stop_immediate(&mut self) {
        self.cancel_increment();
        self.outputs.jog_stop = true;
        self.outputs.jog_stop_immediate = true;
        self.outputs.phase = MotionCommandPhase::StopImmediate;
    }

    fn cancel_increment(&mut self) {
        self.active = None;
        if self.outputs.axis_jog_counts.iter().any(|value| *value != 0)
            || self
                .outputs
                .joint_jog_counts
                .iter()
                .any(|value| *value != 0)
        {
            self.rebase_pending = true;
        }
        self.outputs.axis_jog_enable = [false; 3];
        self.outputs.joint_jog_enable = [false; 3];
        self.outputs.axis_jog_scale = [0.0; 3];
        self.outputs.joint_jog_scale = [0.0; 3];
    }

    pub const fn outputs(&self) -> NativeMotionOutputs {
        self.outputs
    }
}

impl Default for NativeMotionCommandChannel {
    fn default() -> Self {
        Self::new()
    }
}
