use super::*;

const AXES: [AxisSelector; 7] = [
    AxisSelector::Invalid,
    AxisSelector::Off,
    AxisSelector::X,
    AxisSelector::Y,
    AxisSelector::Z,
    AxisSelector::Axis4,
    AxisSelector::Axis5,
];
const MULTIPLIERS: [MultiplierSelector; 5] = [
    MultiplierSelector::Invalid,
    MultiplierSelector::Off,
    MultiplierSelector::X1,
    MultiplierSelector::X10,
    MultiplierSelector::X100,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputClass {
    X10Neutral,
    X1Neutral,
    OffNeutral,
    OffClockwise,
    OffCounterclockwise,
    OffX1ButtonHeld,
    Other,
}

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

fn reachable_states() -> Vec<EstopRecoverySequence> {
    let mut states = [
        RecoveryStage::Inactive,
        RecoveryStage::WaitEstopRelease,
        RecoveryStage::WaitX10,
        RecoveryStage::WaitX1,
        RecoveryStage::WaitOff,
        RecoveryStage::WaitClockwise,
        RecoveryStage::WaitCounterclockwise,
    ]
    .into_iter()
    .map(|stage| EstopRecoverySequence {
        stage,
        clicks: 0,
        quadrature_errors: if stage == RecoveryStage::Inactive {
            None
        } else {
            Some(0)
        },
    })
    .collect::<Vec<_>>();
    for stage in [
        RecoveryStage::WaitButtonPress,
        RecoveryStage::WaitButtonRelease,
    ] {
        for clicks in 0..3 {
            states.push(EstopRecoverySequence {
                stage,
                clicks,
                quadrature_errors: Some(0),
            });
        }
    }
    states.push(EstopRecoverySequence {
        stage: RecoveryStage::CompletePending,
        clicks: 3,
        quadrature_errors: Some(0),
    });
    assert_eq!(states.len(), 14);
    states
}

const fn classify(sample: PendantSample) -> InputClass {
    match (
        sample.axis,
        sample.multiplier,
        sample.selector_valid,
        sample.deadman_held,
        sample.latest_detent,
    ) {
        (AxisSelector::X, MultiplierSelector::X10, true, false, 0) => InputClass::X10Neutral,
        (AxisSelector::X, MultiplierSelector::X1, true, false, 0) => InputClass::X1Neutral,
        (AxisSelector::Off, MultiplierSelector::Off, false, false, 0) => InputClass::OffNeutral,
        (AxisSelector::Off, MultiplierSelector::Off, false, false, 1) => InputClass::OffClockwise,
        (AxisSelector::Off, MultiplierSelector::Off, false, false, -1) => {
            InputClass::OffCounterclockwise
        }
        (AxisSelector::Off, MultiplierSelector::X1, false, true, 0) => InputClass::OffX1ButtonHeld,
        _ => InputClass::Other,
    }
}

fn restarted(mut state: EstopRecoverySequence, sample: PendantSample) -> EstopRecoverySequence {
    state.stage = RecoveryStage::WaitX10;
    state.clicks = 0;
    state.quadrature_errors = Some(sample.quadrature_errors);
    state
}

fn expected_transition(
    mut state: EstopRecoverySequence,
    sample: PendantSample,
) -> (EstopRecoverySequence, RecoveryUpdate) {
    if state.stage == RecoveryStage::Inactive {
        return (
            state,
            RecoveryUpdate {
                stage: state.stage,
                restarted: false,
                unlock_requested: false,
            },
        );
    }
    if sample.estop_pressed {
        state.stage = RecoveryStage::WaitEstopRelease;
        state.clicks = 0;
        state.quadrature_errors = Some(sample.quadrature_errors);
        return (
            state,
            RecoveryUpdate {
                stage: state.stage,
                restarted: false,
                unlock_requested: false,
            },
        );
    }
    if state.quadrature_errors != Some(sample.quadrature_errors)
        || !(-1..=1).contains(&sample.latest_detent)
    {
        state = restarted(state, sample);
        return (
            state,
            RecoveryUpdate {
                stage: state.stage,
                restarted: true,
                unlock_requested: false,
            },
        );
    }

    let class = classify(sample);
    let mut restart_required = false;
    let mut unlock_requested = false;
    match state.stage {
        RecoveryStage::Inactive => unreachable!(),
        RecoveryStage::WaitEstopRelease => {
            state.clicks = 0;
            state.stage = if class == InputClass::X10Neutral {
                RecoveryStage::WaitX1
            } else {
                RecoveryStage::WaitX10
            };
        }
        RecoveryStage::WaitX10 => {
            if class == InputClass::X10Neutral {
                state.stage = RecoveryStage::WaitX1;
            }
        }
        RecoveryStage::WaitX1 => match class {
            InputClass::X10Neutral => {}
            InputClass::X1Neutral => state.stage = RecoveryStage::WaitOff,
            _ => restart_required = true,
        },
        RecoveryStage::WaitOff => match class {
            InputClass::X1Neutral => {}
            InputClass::OffNeutral => state.stage = RecoveryStage::WaitClockwise,
            _ => restart_required = true,
        },
        RecoveryStage::WaitClockwise => match class {
            InputClass::OffNeutral => {}
            InputClass::OffClockwise => state.stage = RecoveryStage::WaitCounterclockwise,
            _ => restart_required = true,
        },
        RecoveryStage::WaitCounterclockwise => match class {
            InputClass::OffNeutral | InputClass::OffClockwise => {}
            InputClass::OffCounterclockwise => state.stage = RecoveryStage::WaitButtonPress,
            _ => restart_required = true,
        },
        RecoveryStage::WaitButtonPress => match class {
            InputClass::OffNeutral => {}
            InputClass::OffCounterclockwise if state.clicks == 0 => {}
            InputClass::OffX1ButtonHeld => state.stage = RecoveryStage::WaitButtonRelease,
            _ => restart_required = true,
        },
        RecoveryStage::WaitButtonRelease => match class {
            InputClass::OffX1ButtonHeld => {}
            InputClass::OffNeutral => {
                state.clicks += 1;
                if state.clicks == 3 {
                    state.stage = RecoveryStage::CompletePending;
                    unlock_requested = true;
                } else {
                    state.stage = RecoveryStage::WaitButtonPress;
                }
            }
            _ => restart_required = true,
        },
        RecoveryStage::CompletePending => {}
    }
    if restart_required {
        state = restarted(state, sample);
    }
    (
        state,
        RecoveryUpdate {
            stage: state.stage,
            restarted: restart_required,
            unlock_requested,
        },
    )
}

#[test]
fn every_reachable_recovery_state_and_physical_input_has_one_exact_transition() {
    let mut cases = 0_usize;
    for initial in reachable_states() {
        for axis in AXES {
            for multiplier in MULTIPLIERS {
                for deadman_held in [false, true] {
                    for estop_pressed in [false, true] {
                        for selector_valid in [false, true] {
                            for latest_detent in -2..=2 {
                                for quadrature_errors in [0, 1] {
                                    let input = PendantSample {
                                        sequence: 1,
                                        quadrature_errors,
                                        latest_detent,
                                        axis,
                                        multiplier,
                                        deadman_held,
                                        estop_pressed,
                                        selector_valid,
                                    };
                                    let (expected_state, expected_update) =
                                        expected_transition(initial, input);
                                    let mut actual_state = initial;
                                    let actual_update = actual_state.process(input);
                                    assert_eq!(actual_state, expected_state, "input={input:?}");
                                    assert_eq!(actual_update, expected_update, "input={input:?}");
                                    cases += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 39_200);
}

#[test]
fn exact_unlock_gesture_is_input_only_and_resets_only_after_acceptance() {
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

    assert!(recovery.active());
    recovery.accept_unlock();
    assert_eq!(recovery, EstopRecoverySequence::new());
}
