use super::super::SupervisorInputs;
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LimitAction {
    None,
    Bounce(usize),
    Fault,
}

const fn decide_limit_action(
    active_motor: Option<usize>,
    toward_positive_limit: bool,
    limits: [bool; 3],
) -> LimitAction {
    if !any(limits) {
        return LimitAction::None;
    }
    match (active_motor, single_active(limits), toward_positive_limit) {
        (Some(active), Some(limit), true) if active == limit => LimitAction::Bounce(limit),
        _ => LimitAction::Fault,
    }
}

impl LinuxCncPendantSupervisor {
    pub(super) fn check_limits(&mut self, inputs: &SupervisorInputs) {
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
        let (active_motor, toward_positive_limit) = self
            .active
            .map(|active| (Some(active.intent.motor), active.toward_positive_limit()))
            .unwrap_or((None, false));
        match decide_limit_action(active_motor, toward_positive_limit, inputs.safety_limits) {
            LimitAction::None => {}
            LimitAction::Bounce(motor) => self.begin_bounce(motor),
            LimitAction::Fault => self.fail(FaultCode::UnexpectedLimit),
        }
    }

    pub(super) fn manage_homing_latches(&mut self, period_ns: u64, inputs: &SupervisorInputs) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_active_motor_direction_and_limit_vector_has_one_exact_action() {
        let mut states = 0_u32;
        for active_motor in [None, Some(0), Some(1), Some(2)] {
            for toward_positive_limit in [false, true] {
                for bits in 0_u8..8 {
                    let limits = [bits & 1 != 0, bits & 2 != 0, bits & 4 != 0];
                    let active_limits = limits.into_iter().filter(|value| *value).count();
                    let expected = if active_limits == 0 {
                        LimitAction::None
                    } else if active_limits == 1
                        && toward_positive_limit
                        && active_motor == single_active(limits)
                    {
                        LimitAction::Bounce(active_motor.expect("matching motor exists"))
                    } else {
                        LimitAction::Fault
                    };
                    assert_eq!(
                        decide_limit_action(active_motor, toward_positive_limit, limits),
                        expected,
                        "active_motor={active_motor:?} toward={toward_positive_limit} limits={limits:?}"
                    );
                    states += 1;
                }
            }
        }
        assert_eq!(states, 64);
    }
}
