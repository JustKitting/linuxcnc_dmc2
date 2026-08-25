use crate::pendant::{AxisSelector, MultiplierSelector, PendantSample};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryStage {
    Inactive,
    WaitEstopRelease,
    WaitX10,
    WaitX1,
    WaitOff,
    WaitClockwise,
    WaitCounterclockwise,
    WaitButtonPress,
    WaitButtonRelease,
    CompletePending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryUpdate {
    pub stage: RecoveryStage,
    pub restarted: bool,
    pub unlock_requested: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EstopRecoverySequence {
    stage: RecoveryStage,
    clicks: u8,
    quadrature_errors: Option<u32>,
}

impl EstopRecoverySequence {
    pub const fn new() -> Self {
        Self {
            stage: RecoveryStage::Inactive,
            clicks: 0,
            quadrature_errors: None,
        }
    }

    pub const fn stage(&self) -> RecoveryStage {
        self.stage
    }

    pub const fn clicks(&self) -> u8 {
        self.clicks
    }

    pub const fn active(&self) -> bool {
        !matches!(self.stage, RecoveryStage::Inactive)
    }

    pub fn engage(&mut self, sample: PendantSample) -> RecoveryUpdate {
        self.stage = RecoveryStage::WaitEstopRelease;
        self.clicks = 0;
        self.quadrature_errors = Some(sample.quadrature_errors);
        self.update(false, false)
    }

    pub fn accept_unlock(&mut self) {
        *self = Self::new();
    }

    fn update(&self, restarted: bool, unlock_requested: bool) -> RecoveryUpdate {
        RecoveryUpdate {
            stage: self.stage,
            restarted,
            unlock_requested,
        }
    }

    pub fn restart(&mut self, sample: PendantSample) {
        self.stage = RecoveryStage::WaitX10;
        self.clicks = 0;
        self.quadrature_errors = Some(sample.quadrature_errors);
    }

    fn restart_update(&mut self, sample: PendantSample) -> RecoveryUpdate {
        self.restart(sample);
        self.update(true, false)
    }

    const fn released_x10(sample: PendantSample) -> bool {
        matches!(sample.axis, AxisSelector::X)
            && matches!(sample.multiplier, MultiplierSelector::X10)
            && sample.selector_valid
            && !sample.deadman_held
    }

    const fn released_x1(sample: PendantSample) -> bool {
        matches!(sample.axis, AxisSelector::X)
            && matches!(sample.multiplier, MultiplierSelector::X1)
            && sample.selector_valid
            && !sample.deadman_held
    }

    const fn released_off(sample: PendantSample) -> bool {
        matches!(sample.axis, AxisSelector::Off)
            && matches!(sample.multiplier, MultiplierSelector::Off)
            && !sample.selector_valid
            && !sample.deadman_held
    }

    const fn off_x1_button_held(sample: PendantSample) -> bool {
        matches!(sample.axis, AxisSelector::Off)
            && matches!(sample.multiplier, MultiplierSelector::X1)
            && !sample.selector_valid
            && sample.deadman_held
    }

    pub fn process(&mut self, sample: PendantSample) -> RecoveryUpdate {
        if !self.active() {
            return self.update(false, false);
        }

        if sample.estop_pressed {
            self.stage = RecoveryStage::WaitEstopRelease;
            self.clicks = 0;
            self.quadrature_errors = Some(sample.quadrature_errors);
            return self.update(false, false);
        }

        match self.quadrature_errors {
            None => self.quadrature_errors = Some(sample.quadrature_errors),
            Some(value) if value != sample.quadrature_errors => return self.restart_update(sample),
            Some(_) => {}
        }

        let signal = sample.latest_detent;
        if signal < -1 || signal > 1 {
            return self.restart_update(sample);
        }

        match self.stage {
            RecoveryStage::Inactive => self.update(false, false),
            RecoveryStage::WaitEstopRelease => {
                self.clicks = 0;
                if Self::released_x10(sample) && signal == 0 {
                    self.stage = RecoveryStage::WaitX1;
                } else {
                    self.stage = RecoveryStage::WaitX10;
                }
                self.update(false, false)
            }
            RecoveryStage::WaitX10 => {
                if Self::released_x10(sample) && signal == 0 {
                    self.stage = RecoveryStage::WaitX1;
                }
                self.update(false, false)
            }
            RecoveryStage::WaitX1 => {
                if Self::released_x10(sample) && signal == 0 {
                    return self.update(false, false);
                }
                if Self::released_x1(sample) && signal == 0 {
                    self.stage = RecoveryStage::WaitOff;
                    return self.update(false, false);
                }
                self.restart_update(sample)
            }
            RecoveryStage::WaitOff => {
                if Self::released_x1(sample) && signal == 0 {
                    return self.update(false, false);
                }
                if Self::released_off(sample) && signal == 0 {
                    self.stage = RecoveryStage::WaitClockwise;
                    return self.update(false, false);
                }
                self.restart_update(sample)
            }
            RecoveryStage::WaitClockwise => {
                if !Self::released_off(sample) {
                    return self.restart_update(sample);
                }
                match signal {
                    0 => self.update(false, false),
                    1 => {
                        self.stage = RecoveryStage::WaitCounterclockwise;
                        self.update(false, false)
                    }
                    _ => self.restart_update(sample),
                }
            }
            RecoveryStage::WaitCounterclockwise => {
                if !Self::released_off(sample) {
                    return self.restart_update(sample);
                }
                match signal {
                    0 | 1 => self.update(false, false),
                    -1 => {
                        self.stage = RecoveryStage::WaitButtonPress;
                        self.update(false, false)
                    }
                    _ => self.restart_update(sample),
                }
            }
            RecoveryStage::WaitButtonPress => {
                if Self::released_off(sample) {
                    if signal == 0 || (signal == -1 && self.clicks == 0) {
                        return self.update(false, false);
                    }
                    return self.restart_update(sample);
                }
                if Self::off_x1_button_held(sample) && signal == 0 {
                    self.stage = RecoveryStage::WaitButtonRelease;
                    return self.update(false, false);
                }
                self.restart_update(sample)
            }
            RecoveryStage::WaitButtonRelease => {
                if Self::off_x1_button_held(sample) && signal == 0 {
                    return self.update(false, false);
                }
                if Self::released_off(sample) && signal == 0 {
                    self.clicks += 1;
                    if self.clicks == 3 {
                        self.stage = RecoveryStage::CompletePending;
                        return self.update(false, true);
                    }
                    self.stage = RecoveryStage::WaitButtonPress;
                    return self.update(false, false);
                }
                self.restart_update(sample)
            }
            RecoveryStage::CompletePending => self.update(false, false),
        }
    }
}

impl Default for EstopRecoverySequence {
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
            axis: AxisSelector::Off,
            multiplier: MultiplierSelector::Off,
            deadman_held: false,
            estop_pressed: false,
            selector_valid: false,
        }
    }

    #[test]
    fn exact_unlock_gesture_is_input_only_and_requests_unlock_after_three_clicks() {
        let mut recovery = EstopRecoverySequence::new();
        let mut pressed = sample(1);
        pressed.estop_pressed = true;
        assert_eq!(
            recovery.engage(pressed).stage,
            RecoveryStage::WaitEstopRelease
        );

        let mut x10 = sample(2);
        x10.axis = AxisSelector::X;
        x10.multiplier = MultiplierSelector::X10;
        x10.selector_valid = true;
        assert_eq!(recovery.process(x10).stage, RecoveryStage::WaitX1);

        let mut x1 = sample(3);
        x1.axis = AxisSelector::X;
        x1.multiplier = MultiplierSelector::X1;
        x1.selector_valid = true;
        assert_eq!(recovery.process(x1).stage, RecoveryStage::WaitOff);

        assert_eq!(
            recovery.process(sample(4)).stage,
            RecoveryStage::WaitClockwise
        );
        let mut clockwise = sample(5);
        clockwise.latest_detent = 1;
        assert_eq!(
            recovery.process(clockwise).stage,
            RecoveryStage::WaitCounterclockwise
        );
        let mut counterclockwise = sample(6);
        counterclockwise.latest_detent = -1;
        assert_eq!(
            recovery.process(counterclockwise).stage,
            RecoveryStage::WaitButtonPress
        );

        for click in 0..3 {
            let mut held = sample(7 + click * 2);
            held.multiplier = MultiplierSelector::X1;
            held.deadman_held = true;
            assert_eq!(
                recovery.process(held).stage,
                RecoveryStage::WaitButtonRelease
            );
            let update = recovery.process(sample(8 + click * 2));
            if click < 2 {
                assert_eq!(update.stage, RecoveryStage::WaitButtonPress);
                assert!(!update.unlock_requested);
            } else {
                assert_eq!(update.stage, RecoveryStage::CompletePending);
                assert!(update.unlock_requested);
            }
        }
    }

    #[test]
    fn wrong_transition_restarts_at_x10_without_unlocking() {
        let mut recovery = EstopRecoverySequence::new();
        let mut pressed = sample(1);
        pressed.estop_pressed = true;
        recovery.engage(pressed);
        let mut x10 = sample(2);
        x10.axis = AxisSelector::X;
        x10.multiplier = MultiplierSelector::X10;
        x10.selector_valid = true;
        recovery.process(x10);

        let wrong = sample(3);
        let update = recovery.process(wrong);
        assert!(update.restarted);
        assert_eq!(update.stage, RecoveryStage::WaitX10);
        assert!(!update.unlock_requested);
        assert_eq!(recovery.clicks(), 0);
    }

    #[test]
    fn quadrature_change_restarts_recovery() {
        let mut recovery = EstopRecoverySequence::new();
        let mut pressed = sample(1);
        pressed.estop_pressed = true;
        recovery.engage(pressed);
        let mut changed = sample(2);
        changed.quadrature_errors = 1;
        let update = recovery.process(changed);
        assert!(update.restarted);
        assert_eq!(update.stage, RecoveryStage::WaitX10);
    }
}
