use super::*;

const AXES: [AxisSelector; 7] = [
    AxisSelector::X,
    AxisSelector::Y,
    AxisSelector::Z,
    AxisSelector::Axis4,
    AxisSelector::Axis5,
    AxisSelector::Off,
    AxisSelector::Invalid,
];
const MULTIPLIERS: [MultiplierSelector; 5] = [
    MultiplierSelector::X1,
    MultiplierSelector::X10,
    MultiplierSelector::X100,
    MultiplierSelector::Off,
    MultiplierSelector::Invalid,
];

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

fn expected_jog(sample: PendantSample) -> Option<JogIntent> {
    if sample.estop_pressed || !sample.selector_valid || !sample.deadman_held {
        return None;
    }
    let axis = sample.axis.motion_axis()?;
    let multiplier = sample.multiplier.motion_multiplier()?;
    JogIntent::from_detent(PendantSelection { axis, multiplier }, sample.latest_detent)
}

fn armed_interpreter(axis: AxisSelector, multiplier: MultiplierSelector) -> PendantInterpreter {
    let arm_axis = if axis.motion_axis().is_some() {
        axis
    } else {
        AxisSelector::X
    };
    let arm_multiplier = if multiplier.motion_multiplier().is_some() {
        multiplier
    } else {
        MultiplierSelector::X1
    };
    let mut interpreter = PendantInterpreter::new();
    interpreter.process(PendantSample {
        sequence: 1,
        axis: arm_axis,
        multiplier: arm_multiplier,
        deadman_held: false,
        ..sample(1)
    });
    interpreter.process(PendantSample {
        sequence: 2,
        axis: arm_axis,
        multiplier: arm_multiplier,
        ..sample(2)
    });
    interpreter
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
fn every_one_of_the_840_physical_packet_states_has_one_exact_decision() {
    let mut states = 0_u32;
    let mut commands = 0_u32;
    for axis in AXES {
        for multiplier in MULTIPLIERS {
            for deadman_held in [false, true] {
                for estop_pressed in [false, true] {
                    for selector_valid in [false, true] {
                        for latest_detent in [-1, 0, 1] {
                            states += 1;
                            let candidate = PendantSample {
                                sequence: 3,
                                latest_detent,
                                axis,
                                multiplier,
                                deadman_held,
                                estop_pressed,
                                selector_valid,
                                ..sample(3)
                            };
                            let expected = expected_jog(candidate);
                            let decision = armed_interpreter(axis, multiplier).process(candidate);
                            match expected {
                                Some(jog) => {
                                    commands += 1;
                                    assert_eq!(decision, PendantDecision::Jog(jog));
                                }
                                None => assert!(!matches!(decision, PendantDecision::Jog(_))),
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(states, 840);
    assert_eq!(commands, 18);
}

#[test]
fn every_sequence_delta_boundary_is_classified_exactly() {
    for (delta, accepted) in [
        (0, false),
        (1, true),
        (i32::MAX as u32, true),
        (i32::MAX as u32 + 1, false),
        (u32::MAX, false),
    ] {
        let mut interpreter = PendantInterpreter::new();
        interpreter.process(sample(0));
        let decision = interpreter.process(sample(delta));
        if accepted {
            assert_eq!(
                decision,
                PendantDecision::Stop(StopReason::SelectionBaseline),
                "delta={delta}"
            );
        } else {
            assert_eq!(
                decision,
                PendantDecision::Stop(StopReason::SequenceRestartedOrRepeated),
                "delta={delta}"
            );
        }
    }
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
fn every_invalid_detent_boundary_faults_closed() {
    for latest_detent in [i32::MIN, -2, 2, i32::MAX] {
        let mut interpreter = armed_interpreter(AxisSelector::X, MultiplierSelector::X1);
        let decision = interpreter.process(PendantSample {
            latest_detent,
            ..sample(3)
        });
        assert_eq!(
            decision,
            PendantDecision::Fault(InterpreterFault::InvalidDetent),
            "latest_detent={latest_detent}"
        );
    }
}

#[test]
fn every_stop_and_fault_reason_has_an_exact_trigger() {
    let cases = [
        (
            PendantSample {
                estop_pressed: true,
                ..sample(2)
            },
            PendantDecision::Stop(StopReason::EstopPressed),
        ),
        (
            PendantSample {
                selector_valid: false,
                ..sample(2)
            },
            PendantDecision::Stop(StopReason::InvalidSelector),
        ),
        (
            PendantSample {
                axis: AxisSelector::Axis4,
                ..sample(2)
            },
            PendantDecision::Stop(StopReason::UnsupportedAxis),
        ),
        (
            PendantSample {
                multiplier: MultiplierSelector::Off,
                ..sample(2)
            },
            PendantDecision::Stop(StopReason::InvalidMultiplier),
        ),
        (
            PendantSample {
                deadman_held: false,
                ..sample(2)
            },
            PendantDecision::Stop(StopReason::DeadmanReleased),
        ),
    ];

    for (case, expected) in cases {
        let mut interpreter = PendantInterpreter::new();
        interpreter.process(sample(1));
        assert_eq!(interpreter.process(case), expected);
    }

    let mut changed = sample(3);
    changed.quadrature_errors = 1;
    assert_eq!(
        armed_interpreter(AxisSelector::X, MultiplierSelector::X1).process(changed),
        PendantDecision::Fault(InterpreterFault::QuadratureErrorChanged)
    );
}

#[test]
fn one_hundred_thousand_detents_never_accumulate_or_change_size() {
    let mut interpreter = armed_interpreter(AxisSelector::X, MultiplierSelector::X100);
    let expected = JogIntent {
        axis: Axis::X,
        motor: 1,
        delta_pulses: -1_000,
        speed_mm_per_minute: 18_000,
    };
    for sequence in 3..100_003 {
        let decision = interpreter.process(PendantSample {
            sequence,
            latest_detent: 1,
            multiplier: MultiplierSelector::X100,
            ..sample(sequence)
        });
        assert_eq!(decision, PendantDecision::Jog(expected));
    }
}
