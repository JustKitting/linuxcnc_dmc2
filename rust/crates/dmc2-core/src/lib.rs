#![cfg_attr(not(test), no_std)]

pub mod halui;
pub mod pendant;
pub mod recovery;
pub mod runtime;
pub mod startup;
pub mod supervisor;

include!(concat!(env!("OUT_DIR"), "/machine_scale.rs"));

pub const MOTOR_BY_AXIS: [usize; 3] = [1, 0, 2];
pub const CLOCKWISE_SIGN_BY_AXIS: [i32; 3] = [-1, 1, 1];
pub const PULSES_PER_MM: i32 = 1_000;
pub const BOUNCE_PULSES: i32 = 50 * MOTOR_PULSE_SCALE;
pub const TASK_HEARTBEAT_TIMEOUT_NS: u64 = 100_000_000;
pub const PENDANT_PACKET_TIMEOUT_NS: u64 = 100_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum Axis {
    X = 0,
    Y = 1,
    Z = 2,
}

impl Axis {
    pub const fn from_wire_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::X),
            1 => Some(Self::Y),
            2 => Some(Self::Z),
            _ => None,
        }
    }

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum Multiplier {
    X1 = 1,
    X10 = 10,
    X100 = 100,
}

impl Multiplier {
    pub const fn from_wire_code(code: i32) -> Option<Self> {
        match code {
            1 => Some(Self::X1),
            10 => Some(Self::X10),
            100 => Some(Self::X100),
            _ => None,
        }
    }

    pub const fn pulses(self) -> i32 {
        match self {
            Self::X1 => 10,
            Self::X10 => 100,
            Self::X100 => 1_000,
        }
    }

    /// Exact previously accepted HALUI jog speed in machine units/minute.
    pub const fn jog_speed_mm_per_minute(self) -> i32 {
        match self {
            Self::X1 => (500 * MOTOR_PULSE_SCALE * 2 * 60) / PULSES_PER_MM,
            Self::X10 => (3_000 * MOTOR_PULSE_SCALE * 5 * 60) / PULSES_PER_MM,
            // Preserve the exact requested 300,000 pulses/s.
            // LinuxCNC motion, not this component, owns enforcement of the
            // accepted 30 mm/s axis maximum.
            Self::X100 => (3_000 * 2 * MOTOR_PULSE_SCALE * 10 * 60) / PULSES_PER_MM,
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
    pub speed_mm_per_minute: i32,
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
            speed_mm_per_minute: selection.multiplier.jog_speed_mm_per_minute(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_user_confirmed_axis_and_motor_mapping_is_preserved() {
        assert_eq!(Axis::X.motor(), 1);
        assert_eq!(Axis::Y.motor(), 0);
        assert_eq!(Axis::Z.motor(), 2);
        assert_eq!(Axis::X.clockwise_machine_sign(), -1);
        assert_eq!(Axis::Y.clockwise_machine_sign(), 1);
        assert_eq!(Axis::Z.clockwise_machine_sign(), 1);
    }

    #[test]
    fn exact_user_confirmed_increment_and_speed_mapping_is_preserved() {
        assert_eq!(Multiplier::X1.pulses(), 10);
        assert_eq!(Multiplier::X10.pulses(), 100);
        assert_eq!(Multiplier::X100.pulses(), 1_000);
        assert_eq!(Multiplier::X1.jog_speed_mm_per_minute(), 300);
        assert_eq!(Multiplier::X10.jog_speed_mm_per_minute(), 4_500);
        assert_eq!(Multiplier::X100.jog_speed_mm_per_minute(), 18_000);
    }

    #[test]
    fn clockwise_and_counterclockwise_intents_are_exact_opposites() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            for multiplier in [Multiplier::X1, Multiplier::X10, Multiplier::X100] {
                let selection = PendantSelection { axis, multiplier };
                let clockwise = JogIntent::from_detent(selection, 1).unwrap();
                let counterclockwise = JogIntent::from_detent(selection, -1).unwrap();
                assert_eq!(clockwise.delta_pulses, -counterclockwise.delta_pulses);
                assert_eq!(clockwise.motor, axis.motor());
                assert_eq!(
                    clockwise.speed_mm_per_minute,
                    multiplier.jog_speed_mm_per_minute()
                );
            }
        }
    }

    #[test]
    fn freshness_fails_closed_at_the_exact_timeout() {
        let mut freshness = Freshness::new();
        assert!(!freshness.is_fresh(100));
        freshness.update(7, 10);
        assert!(freshness.is_fresh(100));
        for _ in 0..9 {
            freshness.update(7, 10);
        }
        assert!(freshness.is_fresh(100));
        freshness.update(7, 10);
        assert!(!freshness.is_fresh(100));
        freshness.update(8, 10);
        assert!(freshness.is_fresh(100));
        assert_eq!(freshness.age_ns(), 0);
    }
}
