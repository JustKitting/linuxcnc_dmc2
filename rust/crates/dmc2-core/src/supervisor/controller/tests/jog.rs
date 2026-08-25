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
