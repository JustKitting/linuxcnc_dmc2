use super::*;
use crate::pendant::AxisSelector;

fn sample_for_axis(sequence: u32, axis: AxisSelector, detent: i32, deadman: bool) -> PendantSample {
    PendantSample {
        axis,
        ..sample(sequence, detent, deadman)
    }
}

fn arm_axis(supervisor: &mut LinuxCncPendantSupervisor, axis: AxisSelector) {
    supervisor.update(1_000_000, inputs(Some(sample_for_axis(1, axis, 0, false))));
    supervisor.update(20_000_000, inputs(Some(sample_for_axis(2, axis, 0, true))));
}

#[test]
fn matching_positive_limit_runs_exact_negative_250_pulse_bounce() {
    let mut supervisor = LinuxCncPendantSupervisor::new();
    arm(&mut supervisor);
    supervisor.update(20_000_000, inputs(Some(sample(3, -1, true))));

    let mut collision = inputs(Some(sample(4, 0, true)));
    collision.machine.axis_stopped[0] = false;
    collision.raw_limits = [false, true, false];
    collision.safety_limits = [false, true, false];
    let stopped = supervisor.update(1_000_000, collision);
    assert_eq!(stopped.command, Some(CommandEvent::JogStopImmediate));
    assert_eq!(stopped.phase, Phase::StoppingBounce);

    let mut bounce = inputs(Some(sample(5, 0, true)));
    bounce.raw_limits = [false, true, false];
    bounce.safety_limits = [false, true, false];
    bounce.counts_by_motor[1] = 2_005;
    bounce.position_feedback_by_motor[1] = 2.005;
    let bounce_output = supervisor.update(25_000_000, bounce);
    assert_eq!(
        bounce_output.command,
        Some(CommandEvent::JogIncrement(JogCommand {
            axis: Axis::X,
            joint_jog: false,
            signed_delta_pulses: -249.5,
            speed_mm_per_minute: 90,
        }))
    );
    assert_eq!(bounce_output.phase, Phase::Bouncing);

    let mut finished = bounce;
    finished.raw_limits = [false; 3];
    finished.counts_by_motor[1] = 1_755;
    finished.position_feedback_by_motor[1] = 1.755;
    let reset = supervisor.update(25_000_000, finished);
    assert_eq!(reset.limit_reset, [false, true, false]);
    assert_eq!(reset.phase, Phase::BounceResetAssert);

    let asserted = supervisor.update(10_000_000, finished);
    assert_eq!(asserted.limit_reset, [false; 3]);
    assert_eq!(asserted.phase, Phase::BounceResetValidate);
    let mut cleared = finished;
    cleared.safety_limits = [false; 3];
    let complete = supervisor.update(10_000_000, cleared);
    assert_eq!(complete.phase, Phase::Idle);
    assert!(!complete.bounce_active);
    assert!(complete.fault.is_none());
}

#[test]
fn non_exact_bounce_count_fails_closed() {
    let mut supervisor = LinuxCncPendantSupervisor::new();
    arm(&mut supervisor);
    supervisor.update(20_000_000, inputs(Some(sample(3, -1, true))));
    let mut collision = inputs(Some(sample(4, 0, true)));
    collision.raw_limits = [false, true, false];
    collision.safety_limits = [false, true, false];
    collision.machine.axis_stopped[0] = false;
    supervisor.update(1_000_000, collision);
    collision.machine.axis_stopped[0] = true;
    collision.counts_by_motor[1] = 2_005;
    collision.position_feedback_by_motor[1] = 2.005;
    supervisor.update(25_000_000, collision);
    collision.raw_limits = [false; 3];
    collision.counts_by_motor[1] = 1_756;
    collision.position_feedback_by_motor[1] = 1.756;
    let output = supervisor.update(25_000_000, collision);
    assert_eq!(output.fault, Some(FaultCode::BounceCountMismatch));
    assert!(!output.external_enable);
}

#[test]
fn wrong_limit_during_jog_faults_without_bounce() {
    let mut supervisor = LinuxCncPendantSupervisor::new();
    arm(&mut supervisor);
    supervisor.update(20_000_000, inputs(Some(sample(3, -1, true))));
    let mut wrong = inputs(Some(sample(4, 0, true)));
    wrong.machine.axis_stopped[0] = false;
    wrong.safety_limits = [true, false, false];
    let output = supervisor.update(1_000_000, wrong);
    assert_eq!(output.fault, Some(FaultCode::UnexpectedLimit));
    assert_eq!(output.command, Some(CommandEvent::JogStopImmediate));
    assert!(!output.external_enable);
}

#[test]
fn every_motor_direction_and_limit_vector_reaches_the_exact_supervisor_path() {
    let cases = [
        (0, Axis::Y, AxisSelector::Y, 1),
        (1, Axis::X, AxisSelector::X, -1),
        (2, Axis::Z, AxisSelector::Z, 1),
    ];
    let mut states = 0_u32;
    for (motor, axis, selector, positive_detent) in cases {
        for toward_positive_limit in [false, true] {
            for bits in 0_u8..8 {
                let limits = [bits & 1 != 0, bits & 2 != 0, bits & 4 != 0];
                let detent = if toward_positive_limit {
                    positive_detent
                } else {
                    -positive_detent
                };
                let mut supervisor = LinuxCncPendantSupervisor::new();
                arm_axis(&mut supervisor, selector);
                let started = supervisor.update(
                    20_000_000,
                    inputs(Some(sample_for_axis(3, selector, detent, true))),
                );
                assert_eq!(started.active_axis, Some(axis));
                assert!(started.command_enable_by_motor[motor]);

                let mut collision = inputs(Some(sample_for_axis(4, selector, 0, true)));
                collision.machine.axis_stopped[axis.index()] = false;
                collision.raw_limits = limits;
                collision.safety_limits = limits;
                let output = supervisor.update(1_000_000, collision);
                let active_limits = limits.into_iter().filter(|value| *value).count();
                if active_limits == 0 {
                    assert!(output.fault.is_none());
                    assert!(!output.bounce_active);
                } else if active_limits == 1 && limits[motor] && toward_positive_limit {
                    assert!(output.fault.is_none());
                    assert_eq!(output.phase, Phase::StoppingBounce);
                    assert_eq!(output.command, Some(CommandEvent::JogStopImmediate));
                } else {
                    assert_eq!(output.fault, Some(FaultCode::UnexpectedLimit));
                    assert!(!output.bounce_active);
                    assert_eq!(output.command, Some(CommandEvent::JogStopImmediate));
                }
                states += 1;
            }
        }
    }
    assert_eq!(states, 48);
}
