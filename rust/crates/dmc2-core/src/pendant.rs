use crate::{Axis, JogIntent, Multiplier, PendantSelection};
use dmc2_diagnostics::diagnostic_catalog;

diagnostic_catalog! {
    pub enum AxisSelector: i32 {
        X = 0 => ("AXIS_X", "x", "the pendant axis selector requests X", "no selector-specific action is required"),
        Y = 1 => ("AXIS_Y", "y", "the pendant axis selector requests Y", "no selector-specific action is required"),
        Z = 2 => ("AXIS_Z", "z", "the pendant axis selector requests Z", "no selector-specific action is required"),
        Axis4 = 3 => ("AXIS_4", "4", "the pendant axis selector requests the reserved fourth axis", "configure that axis before using this selector position"),
        Axis5 = 4 => ("AXIS_5", "5", "the pendant axis selector requests the reserved fifth axis", "configure that axis before using this selector position"),
        Off = -1 => ("AXIS_SELECTOR_OFF", "off", "the pendant axis selector is in its off position", "select a configured axis before requesting a jog"),
        Invalid = -2 => ("AXIS_SELECTOR_INVALID", "invalid", "the pendant axis selector did not decode to one stable position", "place the axis selector in one stable labeled position and inspect its wiring if invalid persists")
    }
}

impl AxisSelector {
    pub const fn motion_axis(self) -> Option<Axis> {
        match self {
            Self::X => Some(Axis::X),
            Self::Y => Some(Axis::Y),
            Self::Z => Some(Axis::Z),
            _ => None,
        }
    }
}

diagnostic_catalog! {
    pub enum MultiplierSelector: i32 {
        X1 = 1 => ("MULTIPLIER_X1", "x1", "the pendant multiplier selector requests the base increment", "confirm the selected increment is appropriate before jogging"),
        X10 = 10 => ("MULTIPLIER_X10", "x10", "the pendant multiplier selector requests ten times the base increment", "confirm the selected increment is appropriate before jogging"),
        X100 = 100 => ("MULTIPLIER_X100", "x100", "the pendant multiplier selector requests one hundred times the base increment", "confirm the selected increment is appropriate before jogging"),
        Off = 0 => ("MULTIPLIER_SELECTOR_OFF", "off", "the pendant multiplier selector is in its off position", "select a configured multiplier before requesting a jog"),
        Invalid = -1 => ("MULTIPLIER_SELECTOR_INVALID", "invalid", "the pendant multiplier selector did not decode to one stable position", "place the multiplier selector in one stable labeled position and inspect its wiring if invalid persists")
    }
}

impl MultiplierSelector {
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
