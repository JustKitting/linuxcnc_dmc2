use super::*;

const PERIODS: [u64; 5] = [0, 1, 1_000_000, 250_000_000, u64::MAX];

fn elapsed_boundaries(limit: u64) -> [u64; 4] {
    [0, limit - 1, limit, u64::MAX]
}

fn expected_heartbeat(mut heartbeat: HeartbeatGenerator, period_ns: u64) -> HeartbeatGenerator {
    heartbeat.elapsed_ns = heartbeat.elapsed_ns.saturating_add(period_ns);
    if heartbeat.elapsed_ns >= HEARTBEAT_TOGGLE_NS {
        heartbeat.value = !heartbeat.value;
        heartbeat.elapsed_ns = 0;
    }
    heartbeat
}

fn expected_mesa(
    mut guard: MesaStartupGuard,
    period_ns: u64,
    servo_thread_ready: bool,
    watchdog_has_bit: bool,
    io_error: bool,
) -> MesaStartupGuard {
    guard.watchdog_clear_requested = false;
    if guard.faulted {
        return guard;
    }
    if io_error {
        guard.phase = MesaStartupPhase::Fault;
        guard.phase_elapsed_ns = 0;
        guard.watchdog_clear_requested = false;
        guard.limit_reset = [false; 3];
        guard.faulted = true;
        return guard;
    }
    if guard.phase == MesaStartupPhase::Ready {
        if watchdog_has_bit {
            guard.phase = MesaStartupPhase::Fault;
            guard.phase_elapsed_ns = 0;
            guard.limit_reset = [false; 3];
            guard.faulted = true;
        }
        return guard;
    }
    if guard.phase == MesaStartupPhase::WaitServo {
        if !servo_thread_ready {
            return guard;
        }
        guard.phase = MesaStartupPhase::WatchdogStable;
        guard.phase_elapsed_ns = 0;
    }
    if watchdog_has_bit {
        guard.phase = MesaStartupPhase::WatchdogStable;
        guard.phase_elapsed_ns = 0;
        guard.limit_reset = [false; 3];
        guard.watchdog_clear_requested = true;
        return guard;
    }

    guard.phase_elapsed_ns = guard.phase_elapsed_ns.saturating_add(period_ns);
    let transition = match guard.phase {
        MesaStartupPhase::WatchdogStable if guard.phase_elapsed_ns >= MESA_WATCHDOG_STABLE_NS => {
            Some((MesaStartupPhase::LimitReset, [true; 3]))
        }
        MesaStartupPhase::LimitReset if guard.phase_elapsed_ns >= LIMIT_RESET_NS => {
            Some((MesaStartupPhase::LimitSettle, [false; 3]))
        }
        MesaStartupPhase::LimitSettle if guard.phase_elapsed_ns >= LIMIT_SETTLE_NS => {
            Some((MesaStartupPhase::Ready, [false; 3]))
        }
        _ => None,
    };
    if let Some((phase, limit_reset)) = transition {
        guard.phase = phase;
        guard.phase_elapsed_ns = 0;
        guard.limit_reset = limit_reset;
    }
    guard
}

fn expected_watchdog(
    mut guard: ControllerWatchdogGuard,
    period_ns: u64,
    watchdog_ok: bool,
    prerequisites_ready: bool,
) -> ControllerWatchdogGuard {
    let wait_for_prerequisites = |guard: &mut ControllerWatchdogGuard| {
        guard.phase = ControllerWatchdogPhase::WaitPrerequisites;
        guard.phase_elapsed_ns = 0;
        guard.enable = false;
    };
    let begin_low = |guard: &mut ControllerWatchdogGuard| {
        guard.phase = ControllerWatchdogPhase::ArmLow;
        guard.phase_elapsed_ns = 0;
        guard.enable = false;
    };
    if guard.faulted {
        return guard;
    }
    if guard.phase == ControllerWatchdogPhase::Ready {
        if !guard.runtime_committed && !prerequisites_ready {
            wait_for_prerequisites(&mut guard);
        } else if !watchdog_ok {
            if guard.runtime_committed {
                guard.phase = ControllerWatchdogPhase::Fault;
                guard.phase_elapsed_ns = 0;
                guard.enable = false;
                guard.faulted = true;
            } else {
                begin_low(&mut guard);
            }
        }
        return guard;
    }
    if !prerequisites_ready {
        if guard.phase != ControllerWatchdogPhase::WaitPrerequisites {
            wait_for_prerequisites(&mut guard);
        }
        return guard;
    }

    guard.phase_elapsed_ns = guard.phase_elapsed_ns.saturating_add(period_ns);
    match guard.phase {
        ControllerWatchdogPhase::WaitPrerequisites => begin_low(&mut guard),
        ControllerWatchdogPhase::ArmLow
            if guard.phase_elapsed_ns >= CONTROLLER_WATCHDOG_ARM_LOW_NS =>
        {
            guard.enable = true;
            guard.arm_attempts = guard.arm_attempts.wrapping_add(1);
            guard.phase = ControllerWatchdogPhase::WaitOk;
            guard.phase_elapsed_ns = 0;
        }
        ControllerWatchdogPhase::WaitOk if watchdog_ok => {
            guard.phase = ControllerWatchdogPhase::Stable;
            guard.phase_elapsed_ns = 0;
        }
        ControllerWatchdogPhase::WaitOk
            if guard.phase_elapsed_ns >= CONTROLLER_WATCHDOG_OK_WAIT_NS =>
        {
            begin_low(&mut guard);
        }
        ControllerWatchdogPhase::Stable if !watchdog_ok => begin_low(&mut guard),
        ControllerWatchdogPhase::Stable
            if guard.phase_elapsed_ns >= CONTROLLER_WATCHDOG_STABLE_NS =>
        {
            guard.phase = ControllerWatchdogPhase::Ready;
            guard.phase_elapsed_ns = 0;
        }
        _ => {}
    }
    guard
}

#[test]
fn every_heartbeat_level_elapsed_boundary_and_period_has_one_exact_update() {
    let mut cases = 0;
    for value in [false, true] {
        for elapsed_ns in elapsed_boundaries(HEARTBEAT_TOGGLE_NS) {
            for period_ns in PERIODS {
                let initial = HeartbeatGenerator { value, elapsed_ns };
                let expected = expected_heartbeat(initial, period_ns);
                let mut actual = initial;
                assert_eq!(actual.update(period_ns), expected.value);
                assert_eq!(actual, expected);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 40);
}

#[test]
fn every_reachable_mesa_phase_input_and_timing_boundary_has_one_exact_update() {
    let phases = [
        (MesaStartupPhase::WaitServo, MESA_WATCHDOG_STABLE_NS),
        (MesaStartupPhase::WatchdogStable, MESA_WATCHDOG_STABLE_NS),
        (MesaStartupPhase::LimitReset, LIMIT_RESET_NS),
        (MesaStartupPhase::LimitSettle, LIMIT_SETTLE_NS),
        (MesaStartupPhase::Ready, LIMIT_SETTLE_NS),
        (MesaStartupPhase::Fault, LIMIT_SETTLE_NS),
    ];
    let mut cases = 0;
    for (phase, threshold) in phases {
        for elapsed_ns in elapsed_boundaries(threshold) {
            for period_ns in PERIODS {
                for servo_thread_ready in [false, true] {
                    for watchdog_has_bit in [false, true] {
                        for io_error in [false, true] {
                            let initial = MesaStartupGuard {
                                phase,
                                phase_elapsed_ns: elapsed_ns,
                                watchdog_clear_requested: false,
                                limit_reset: if phase == MesaStartupPhase::LimitReset {
                                    [true; 3]
                                } else {
                                    [false; 3]
                                },
                                faulted: phase == MesaStartupPhase::Fault,
                            };
                            let expected = expected_mesa(
                                initial,
                                period_ns,
                                servo_thread_ready,
                                watchdog_has_bit,
                                io_error,
                            );
                            let mut actual = initial;
                            actual.update(
                                period_ns,
                                servo_thread_ready,
                                watchdog_has_bit,
                                io_error,
                            );
                            assert_eq!(actual, expected);
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 960);
}

#[test]
fn every_reachable_watchdog_phase_input_and_timing_boundary_has_one_exact_update() {
    let phases = [
        (ControllerWatchdogPhase::WaitPrerequisites, false),
        (ControllerWatchdogPhase::ArmLow, false),
        (ControllerWatchdogPhase::WaitOk, false),
        (ControllerWatchdogPhase::Stable, false),
        (ControllerWatchdogPhase::Ready, false),
        (ControllerWatchdogPhase::Ready, true),
        (ControllerWatchdogPhase::Fault, true),
    ];
    let mut cases = 0;
    for (phase, runtime_committed) in phases {
        let threshold = match phase {
            ControllerWatchdogPhase::ArmLow => CONTROLLER_WATCHDOG_ARM_LOW_NS,
            ControllerWatchdogPhase::WaitOk => CONTROLLER_WATCHDOG_OK_WAIT_NS,
            ControllerWatchdogPhase::Stable => CONTROLLER_WATCHDOG_STABLE_NS,
            _ => CONTROLLER_WATCHDOG_ARM_LOW_NS,
        };
        for elapsed_ns in elapsed_boundaries(threshold) {
            for period_ns in PERIODS {
                for watchdog_ok in [false, true] {
                    for prerequisites_ready in [false, true] {
                        for arm_attempts in [0, u32::MAX] {
                            let initial = ControllerWatchdogGuard {
                                phase,
                                phase_elapsed_ns: elapsed_ns,
                                enable: matches!(
                                    phase,
                                    ControllerWatchdogPhase::WaitOk
                                        | ControllerWatchdogPhase::Stable
                                        | ControllerWatchdogPhase::Ready
                                ),
                                runtime_committed,
                                faulted: phase == ControllerWatchdogPhase::Fault,
                                arm_attempts,
                            };
                            let expected = expected_watchdog(
                                initial,
                                period_ns,
                                watchdog_ok,
                                prerequisites_ready,
                            );
                            let mut actual = initial;
                            actual.update(period_ns, watchdog_ok, prerequisites_ready);
                            assert_eq!(actual, expected);
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 1_120);
}

#[test]
fn runtime_commit_is_accepted_only_from_nonfaulted_ready() {
    for phase in [
        ControllerWatchdogPhase::WaitPrerequisites,
        ControllerWatchdogPhase::ArmLow,
        ControllerWatchdogPhase::WaitOk,
        ControllerWatchdogPhase::Stable,
        ControllerWatchdogPhase::Ready,
        ControllerWatchdogPhase::Fault,
    ] {
        let mut guard = ControllerWatchdogGuard {
            phase,
            phase_elapsed_ns: 0,
            enable: phase == ControllerWatchdogPhase::Ready,
            runtime_committed: false,
            faulted: phase == ControllerWatchdogPhase::Fault,
            arm_attempts: 0,
        };
        let expected = phase == ControllerWatchdogPhase::Ready;
        assert_eq!(guard.commit_runtime(), expected);
        assert_eq!(guard.runtime_committed, expected);
    }
}
