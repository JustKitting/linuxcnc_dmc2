use crate::{Axis, JogIntent, Multiplier, PendantSelection};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum AxisSelector {
    Invalid = -2,
    Off = -1,
    X = 0,
    Y = 1,
    Z = 2,
    Axis4 = 3,
    Axis5 = 4,
}

impl AxisSelector {
    pub const fn from_wire_code(code: i32) -> Option<Self> {
        match code {
            -2 => Some(Self::Invalid),
            -1 => Some(Self::Off),
            0 => Some(Self::X),
            1 => Some(Self::Y),
            2 => Some(Self::Z),
            3 => Some(Self::Axis4),
            4 => Some(Self::Axis5),
            _ => None,
        }
    }

    pub const fn motion_axis(self) -> Option<Axis> {
        match self {
            Self::X => Some(Axis::X),
            Self::Y => Some(Axis::Y),
            Self::Z => Some(Axis::Z),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum MultiplierSelector {
    Invalid = -1,
    Off = 0,
    X1 = 1,
    X10 = 10,
    X100 = 100,
}

impl MultiplierSelector {
    pub const fn from_wire_code(code: i32) -> Option<Self> {
        match code {
            -1 => Some(Self::Invalid),
            0 => Some(Self::Off),
            1 => Some(Self::X1),
            10 => Some(Self::X10),
            100 => Some(Self::X100),
            _ => None,
        }
    }

    pub const fn motion_multiplier(self) -> Option<Multiplier> {
        match self {
            Self::X1 => Some(Multiplier::X1),
            Self::X10 => Some(Multiplier::X10),
            Self::X100 => Some(Multiplier::X100),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendantSample {
    pub sequence: u32,
    pub quadrature_errors: u32,
    pub latest_detent: i32,
    pub axis: AxisSelector,
    pub multiplier: MultiplierSelector,
    pub deadman_held: bool,
    pub estop_pressed: bool,
    pub selector_valid: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopReason {
    InitialBaseline,
    SequenceRestartedOrRepeated,
    EstopPressed,
    InvalidSelector,
    UnsupportedAxis,
    InvalidMultiplier,
    DeadmanReleased,
    SelectionBaseline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterpreterFault {
    QuadratureErrorChanged,
    InvalidDetent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PendantDecision {
    Stop(StopReason),
    NoDetent,
    Jog(JogIntent),
    Fault(InterpreterFault),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendantInterpreter {
    previous_sequence: Option<u32>,
    previous_quadrature_errors: Option<u32>,
    armed_selection: Option<PendantSelection>,
}

impl PendantInterpreter {
    pub const fn new() -> Self {
        Self {
            previous_sequence: None,
            previous_quadrature_errors: None,
            armed_selection: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    fn stop(&mut self, reason: StopReason) -> PendantDecision {
        self.armed_selection = None;
        PendantDecision::Stop(reason)
    }

    pub fn process(&mut self, sample: PendantSample) -> PendantDecision {
        let previous_sequence = self.previous_sequence;
        let previous_quadrature_errors = self.previous_quadrature_errors;
        self.previous_sequence = Some(sample.sequence);
        self.previous_quadrature_errors = Some(sample.quadrature_errors);

        let Some(previous_sequence) = previous_sequence else {
            return self.stop(StopReason::InitialBaseline);
        };

        let sequence_delta = sample.sequence.wrapping_sub(previous_sequence);
        if sequence_delta == 0 || sequence_delta > i32::MAX as u32 {
            return self.stop(StopReason::SequenceRestartedOrRepeated);
        }

        if previous_quadrature_errors != Some(sample.quadrature_errors) {
            self.armed_selection = None;
            return PendantDecision::Fault(InterpreterFault::QuadratureErrorChanged);
        }
        if sample.latest_detent < -1 || sample.latest_detent > 1 {
            self.armed_selection = None;
            return PendantDecision::Fault(InterpreterFault::InvalidDetent);
        }
        if sample.estop_pressed {
            return self.stop(StopReason::EstopPressed);
        }
        if !sample.selector_valid {
            return self.stop(StopReason::InvalidSelector);
        }
        let Some(axis) = sample.axis.motion_axis() else {
            return self.stop(StopReason::UnsupportedAxis);
        };
        let Some(multiplier) = sample.multiplier.motion_multiplier() else {
            return self.stop(StopReason::InvalidMultiplier);
        };
        if !sample.deadman_held {
            return self.stop(StopReason::DeadmanReleased);
        }

        let selection = PendantSelection { axis, multiplier };
        if self.armed_selection != Some(selection) {
            self.armed_selection = Some(selection);
            return PendantDecision::Stop(StopReason::SelectionBaseline);
        }
        if sample.latest_detent == 0 {
            return PendantDecision::NoDetent;
        }

        match JogIntent::from_detent(selection, sample.latest_detent) {
            Some(intent) => PendantDecision::Jog(intent),
            None => PendantDecision::Fault(InterpreterFault::InvalidDetent),
        }
    }
}

impl Default for PendantInterpreter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(sequence: u32) -> PendantSample {
        PendantSample {
            sequence,
            quadrature_errors: 0,
            latest_detent: 0,
            axis: AxisSelector::X,
            multiplier: MultiplierSelector::X1,
            deadman_held: true,
            estop_pressed: false,
            selector_valid: true,
        }
    }

    #[test]
    fn first_packet_and_selection_edge_can_never_jog() {
        let mut interpreter = PendantInterpreter::new();
        let mut first = sample(10);
        first.latest_detent = 1;
        assert_eq!(
            interpreter.process(first),
            PendantDecision::Stop(StopReason::InitialBaseline)
        );

        let mut second = sample(11);
        second.latest_detent = 1;
        assert_eq!(
            interpreter.process(second),
            PendantDecision::Stop(StopReason::SelectionBaseline)
        );

        let mut third = sample(12);
        third.latest_detent = 1;
        assert_eq!(
            interpreter.process(third),
            PendantDecision::Jog(JogIntent {
                axis: Axis::X,
                motor: 1,
                delta_pulses: -10,
                speed_mm_per_minute: 300,
            })
        );
    }

    #[test]
    fn repeated_restarted_or_quadrature_changed_packets_fail_closed() {
        let mut interpreter = PendantInterpreter::new();
        assert!(matches!(
            interpreter.process(sample(10)),
            PendantDecision::Stop(StopReason::InitialBaseline)
        ));
        assert_eq!(
            interpreter.process(sample(10)),
            PendantDecision::Stop(StopReason::SequenceRestartedOrRepeated)
        );

        let mut changed = sample(11);
        changed.quadrature_errors = 1;
        assert_eq!(
            interpreter.process(changed),
            PendantDecision::Fault(InterpreterFault::QuadratureErrorChanged)
        );
    }

    #[test]
    fn u32_sequence_wrap_is_forward_progress() {
        let mut interpreter = PendantInterpreter::new();
        interpreter.process(sample(u32::MAX));
        assert_eq!(
            interpreter.process(sample(0)),
            PendantDecision::Stop(StopReason::SelectionBaseline)
        );
    }

    #[test]
    fn every_non_commanding_selector_or_deadman_state_stops() {
        let cases = [
            (
                PendantSample {
                    selector_valid: false,
                    ..sample(2)
                },
                StopReason::InvalidSelector,
            ),
            (
                PendantSample {
                    axis: AxisSelector::Axis4,
                    ..sample(2)
                },
                StopReason::UnsupportedAxis,
            ),
            (
                PendantSample {
                    multiplier: MultiplierSelector::Off,
                    ..sample(2)
                },
                StopReason::InvalidMultiplier,
            ),
            (
                PendantSample {
                    deadman_held: false,
                    ..sample(2)
                },
                StopReason::DeadmanReleased,
            ),
        ];

        for (case, expected) in cases {
            let mut interpreter = PendantInterpreter::new();
            interpreter.process(sample(1));
            assert_eq!(interpreter.process(case), PendantDecision::Stop(expected));
        }
    }
}
