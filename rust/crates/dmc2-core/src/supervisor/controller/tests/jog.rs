use super::*;

#[test]
fn clockwise_x_is_one_exact_negative_ten_pulse_linuxcnc_request() {
    let mut supervisor = LinuxCncPendantSupervisor::new();
    arm(&mut supervisor);
    let output = supervisor.update(20_000_000, inputs(Some(sample(3, 1, true))));
    assert_eq!(
        output.command,
        Some(CommandEvent::JogIncrement(JogCommand {
            axis: Axis::X,
            joint_jog: false,
            signed_delta_pulses: -10.0,
            speed_mm_per_minute: 300,
        }))
    );
    assert_eq!(output.active_axis, Some(Axis::X));
    assert!(output.command_enable_by_motor[1]);
    assert!(!output.toward_limit_by_motor[1]);
}

#[test]
fn deadman_release_requests_linuxcnc_jog_stop() {
    let mut supervisor = LinuxCncPendantSupervisor::new();
    arm(&mut supervisor);
    supervisor.update(20_000_000, inputs(Some(sample(3, 1, true))));
    let mut released = inputs(Some(sample(4, 0, false)));
    released.machine.axis_stopped[0] = false;
    let output = supervisor.update(1_000_000, released);
    assert_eq!(output.command, Some(CommandEvent::JogStop));
    assert_eq!(output.phase, Phase::StoppingCancel);
}

#[test]
fn completed_increment_accepts_an_exact_integer_position_target() {
    let mut supervisor = LinuxCncPendantSupervisor::new();
    arm(&mut supervisor);
    supervisor.update(20_000_000, inputs(Some(sample(3, 1, true))));

    let mut completed = inputs(Some(sample(4, 0, true)));
    completed.counts_by_motor[1] = 1_990;
    completed.position_feedback_by_motor[1] = 1.990;
    let output = supervisor.update(25_000_000, completed);

    assert!(output.fault.is_none());
    assert!(!output.jog_active);
}

#[test]
fn completed_negative_increment_accepts_the_exact_live_hostmot2_fractional_feedback() {
    let mut supervisor = LinuxCncPendantSupervisor::new();
    arm(&mut supervisor);
    let mut start = inputs(Some(sample(3, 1, true)));
    start.counts_by_motor[1] = 0;
    start.position_feedback_by_motor[1] = 0.0;
    supervisor.update(20_000_000, start);

    let mut completed = inputs(Some(sample(4, 0, true)));
    // Exact live failure capture: HostMot2's 16.16 accumulator represented
    // -10.00316 generated pulses. Its arithmetic-shifted count is -11.
    completed.counts_by_motor[1] = -11;
    completed.position_feedback_by_motor[1] = -0.010_003_16;
    let output = supervisor.update(25_000_000, completed);

    assert!(output.fault.is_none());
    assert!(!output.jog_active);
}

#[test]
fn completed_positive_increment_accepts_hostmot2_fractional_feedback() {
    let mut supervisor = LinuxCncPendantSupervisor::new();
    arm(&mut supervisor);
    let mut start = inputs(Some(sample(3, -1, true)));
    start.counts_by_motor[1] = 0;
    start.position_feedback_by_motor[1] = 0.0;
    supervisor.update(20_000_000, start);

    let mut completed = inputs(Some(sample(4, 0, true)));
    completed.counts_by_motor[1] = 10;
    completed.position_feedback_by_motor[1] = 0.010_003_16;
    let output = supervisor.update(25_000_000, completed);

    assert!(output.fault.is_none());
    assert!(!output.jog_active);
}

#[test]
fn incoherent_hostmot2_count_and_position_fail_closed() {
    let mut supervisor = LinuxCncPendantSupervisor::new();
    arm(&mut supervisor);
    let mut start = inputs(Some(sample(3, 1, true)));
    start.counts_by_motor[1] = 0;
    start.position_feedback_by_motor[1] = 0.0;
    supervisor.update(20_000_000, start);

    let mut completed = inputs(Some(sample(4, 0, true)));
    completed.counts_by_motor[1] = -10;
    completed.position_feedback_by_motor[1] = -0.010_003_16;
    let output = supervisor.update(25_000_000, completed);

    assert_eq!(output.fault, Some(FaultCode::JogFeedbackIncoherent));
    assert_eq!(output.command, Some(CommandEvent::JogStopImmediate));
    assert!(!output.external_enable);
}

#[test]
fn non_finite_hostmot2_position_fails_before_any_jog_command() {
    let mut supervisor = LinuxCncPendantSupervisor::new();
    arm(&mut supervisor);
    let mut start = inputs(Some(sample(3, 1, true)));
    start.position_feedback_by_motor[1] = f64::NAN;
    let output = supervisor.update(20_000_000, start);

    assert_eq!(output.fault, Some(FaultCode::JogFeedbackUnavailable));
    assert_eq!(output.command, Some(CommandEvent::JogStopImmediate));
    assert!(!output.external_enable);
}

#[test]
fn missed_increment_fails_closed_instead_of_reporting_success() {
    let mut supervisor = LinuxCncPendantSupervisor::new();
    arm(&mut supervisor);
    supervisor.update(20_000_000, inputs(Some(sample(3, 1, true))));

    let output = supervisor.update(25_000_000, inputs(Some(sample(4, 0, true))));

    assert_eq!(output.fault, Some(FaultCode::JogCountMismatch));
    assert_eq!(output.command, Some(CommandEvent::JogStopImmediate));
    assert!(!output.external_enable);
}

#[test]
fn reversal_storage_is_one_replaceable_slot() {
    let mut supervisor = LinuxCncPendantSupervisor::new();
    arm(&mut supervisor);
    supervisor.update(20_000_000, inputs(Some(sample(3, 1, true))));

    let mut moving = inputs(Some(sample(4, -1, true)));
    moving.machine.axis_stopped[0] = false;
    let first = supervisor.update(1_000_000, moving);
    assert_eq!(first.command, Some(CommandEvent::JogStop));
    assert_eq!(first.phase, Phase::StoppingReplace);

    let mut replaced = inputs(Some(sample(5, 1, true)));
    replaced.machine.axis_stopped[0] = false;
    let second = supervisor.update(1_000_000, replaced);
    assert_eq!(second.command, None);
    assert_eq!(second.phase, Phase::StoppingReplace);

    let completed = supervisor.update(25_000_000, inputs(Some(sample(6, 0, true))));
    assert_eq!(
        completed.command,
        Some(CommandEvent::JogIncrement(JogCommand {
            axis: Axis::X,
            joint_jog: false,
            signed_delta_pulses: -10.0,
            speed_mm_per_minute: 300,
        }))
    );
}

#[test]
fn one_hundred_thousand_detents_keep_only_active_and_one_replaceable_pending_target() {
    let mut same_direction = LinuxCncPendantSupervisor::new();
    arm(&mut same_direction);
    same_direction.update(20_000_000, inputs(Some(sample(3, 1, true))));
    for sequence in 4..100_004 {
        let mut moving = inputs(Some(sample(sequence, 1, true)));
        moving.machine.axis_stopped[Axis::X.index()] = false;
        let output = same_direction.update(1_000_000, moving);
        assert!(output.command.is_none());
        assert!(output.fault.is_none());
    }
    assert!(same_direction.active.is_some());
    assert!(same_direction.pending.is_none());
    assert_eq!(same_direction.phase, Phase::Idle);

    let mut reversals = LinuxCncPendantSupervisor::new();
    arm(&mut reversals);
    reversals.update(20_000_000, inputs(Some(sample(3, 1, true))));
    let mut stop_commands = 0_u32;
    let mut increment_commands = 0_u32;
    for sequence in 4..100_004 {
        let detent = if sequence & 1 == 0 { -1 } else { 1 };
        let mut moving = inputs(Some(sample(sequence, detent, true)));
        moving.machine.axis_stopped[Axis::X.index()] = false;
        let output = reversals.update(1_000_000, moving);
        match output.command {
            Some(CommandEvent::JogStop) => stop_commands += 1,
            Some(CommandEvent::JogIncrement(_)) => increment_commands += 1,
            Some(CommandEvent::JogStopImmediate) | None => {}
        }
        assert!(output.fault.is_none());
    }
    assert_eq!(stop_commands, 1);
    assert_eq!(increment_commands, 0);
    assert!(reversals.active.is_some());
    assert!(reversals.pending.is_some());
    assert_eq!(reversals.phase, Phase::StoppingReplace);
}
