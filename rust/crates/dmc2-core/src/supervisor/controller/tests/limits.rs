use super::*;

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
