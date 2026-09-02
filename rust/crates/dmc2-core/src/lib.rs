#![cfg_attr(not(test), no_std)]

use dmc2_diagnostics::diagnostic_catalog;

pub mod motion;
pub mod pendant;
pub mod recovery;
pub mod runtime;
pub mod startup;
pub mod supervisor;

include!(concat!(env!("OUT_DIR"), "/machine_scale.rs"));

pub const MOTOR_BY_AXIS: [usize; 3] = [1, 0, 2];
pub const CLOCKWISE_SIGN_BY_AXIS: [i32; 3] = [-1, 1, 1];
pub const TASK_HEARTBEAT_TIMEOUT_NS: u64 = 100_000_000;
pub const PENDANT_PACKET_TIMEOUT_NS: u64 = 100_000_000;

diagnostic_catalog! {
    pub enum Axis: i32 {
        X = 0 => ("AXIS_X", "x", "the decoded motion axis is X", "confirm this named axis matches the physical motion requested before jogging"),
        Y = 1 => ("AXIS_Y", "y", "the decoded motion axis is Y", "confirm this named axis matches the physical motion requested before jogging"),
        Z = 2 => ("AXIS_Z", "z", "the decoded motion axis is Z", "confirm this named axis matches the physical motion requested before jogging")
    }
}

impl Axis {
    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn motor(self) -> usize {
        MOTOR_BY_AXIS[self.index()]
    }

    pub const fn clockwise_machine_sign(self) -> i32 {
        CLOCKWISE_SIGN_BY_AXIS[self.index()]
    }
}

diagnostic_catalog! {
    pub enum Multiplier: i32 {
        X1 = 1 => ("MULTIPLIER_X1", "x1", "the decoded jog increment is the base multiplier", "confirm this named increment is appropriate before jogging"),
        X10 = 10 => ("MULTIPLIER_X10", "x10", "the decoded jog increment is ten times the base multiplier", "confirm this named increment is appropriate before jogging"),
        X100 = 100 => ("MULTIPLIER_X100", "x100", "the decoded jog increment is one hundred times the base multiplier", "confirm this named increment is appropriate before jogging")
    }
}

impl Multiplier {
    pub const fn pulses(self) -> i32 {
        match self {
            Self::X1 => PENDANT_INCREMENT_PULSES[0],
            Self::X10 => PENDANT_INCREMENT_PULSES[1],
            Self::X100 => PENDANT_INCREMENT_PULSES[2],
        }
    }

    /// Exact accepted rate for issuing the finite position target. LinuxCNC's
    /// native planner separately owns acceleration and physical velocity.
    pub const fn jog_target_rate_mm_per_minute(self) -> i32 {
        match self {
            Self::X1 => PENDANT_TARGET_PULSES_PER_SECOND[0] * 60 / PULSES_PER_MM,
            Self::X10 => PENDANT_TARGET_PULSES_PER_SECOND[1] * 60 / PULSES_PER_MM,
            Self::X100 => PENDANT_TARGET_PULSES_PER_SECOND[2] * 60 / PULSES_PER_MM,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Freshness {
    last_value: u32,
    age_ns: u64,
    initialized: bool,
}

impl Freshness {
    pub const fn new() -> Self {
        Self {
            last_value: 0,
            age_ns: 0,
            initialized: false,
        }
    }

    pub fn update(&mut self, value: u32, period_ns: u64) {
        if !self.initialized || value != self.last_value {
            self.last_value = value;
            self.age_ns = 0;
            self.initialized = true;
        } else {
            self.age_ns = self.age_ns.saturating_add(period_ns);
        }
    }

    pub fn elapse(&mut self, period_ns: u64) {
        if self.initialized {
            self.age_ns = self.age_ns.saturating_add(period_ns);
        }
    }

    pub const fn is_fresh(&self, timeout_ns: u64) -> bool {
        self.initialized && self.age_ns < timeout_ns
    }

    pub const fn age_ns(&self) -> u64 {
        self.age_ns
    }
}

impl Default for Freshness {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendantSelection {
    pub axis: Axis,
    pub multiplier: Multiplier,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JogIntent {
    pub axis: Axis,
    pub motor: usize,
    pub delta_pulses: i32,
    pub target_rate_mm_per_minute: i32,
}

impl JogIntent {
    pub const fn from_detent(selection: PendantSelection, clockwise_detent: i32) -> Option<Self> {
        if clockwise_detent != -1 && clockwise_detent != 1 {
            return None;
        }
        let delta_pulses = clockwise_detent
            * selection.axis.clockwise_machine_sign()
            * selection.multiplier.pulses();
        Some(Self {
            axis: selection.axis,
            motor: selection.axis.motor(),
            delta_pulses,
            target_rate_mm_per_minute: selection.multiplier.jog_target_rate_mm_per_minute(),
        })
    }
}
