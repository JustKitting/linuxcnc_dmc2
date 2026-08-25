use super::*;

fn task_snapshot(seed: u32) -> CachedTaskSnapshot {
    CachedTaskSnapshot {
        connected: seed & 1 != 0,
        fault: seed & 2 != 0,
        heartbeat: seed,
        machine: MachineSnapshot {
            machine_on: seed & 4 != 0,
            estopped: seed & 8 != 0,
            manual_mode: seed & 16 != 0,
            joint_mode: seed & 32 != 0,
            teleop_mode: seed & 64 != 0,
            interp_idle: seed & 128 != 0,
            homed: [seed & 256 != 0, seed & 512 != 0, seed & 1_024 != 0],
            homing: [seed & 2_048 != 0, seed & 4_096 != 0, seed & 8_192 != 0],
            axis_stopped: [seed & 16_384 != 0, seed & 32_768 != 0, seed & 65_536 != 0],
        },
    }
}

#[test]
fn every_eight_bit_generation_pair_has_one_exact_coherence_decision() {
    let baseline = task_snapshot(0x15_555);
    let candidate = task_snapshot(0x0a_aaa);

    for first in 0_u32..=u8::MAX.into() {
        for second in 0_u32..=u8::MAX.into() {
            let expected_coherent = first == second && second & 1 == 0;
            assert_eq!(generation_is_coherent(first, second), expected_coherent);

            let mut cached = baseline;
            commit_task_snapshot(&mut cached, first, second, candidate);
            assert_eq!(
                cached,
                if expected_coherent {
                    candidate
                } else {
                    baseline
                }
            );
        }
    }
}

#[test]
fn full_width_generation_boundaries_are_classified_without_wrap_assumptions() {
    for (first, second, expected) in [
        (0, 0, true),
        (1, 1, false),
        (u32::MAX - 1, u32::MAX - 1, true),
        (u32::MAX, u32::MAX, false),
        (0, u32::MAX - 1, false),
        (u32::MAX - 1, 0, false),
    ] {
        assert_eq!(generation_is_coherent(first, second), expected);
    }
}

#[test]
fn incoherent_pendant_fallback_is_fully_fail_closed() {
    assert_eq!(
        safe_pendant(),
        PendantSample {
            sequence: 0,
            quadrature_errors: 0,
            latest_detent: 0,
            axis: AxisSelector::Invalid,
            multiplier: MultiplierSelector::Invalid,
            deadman_held: false,
            estop_pressed: true,
            selector_valid: false,
        }
    );
}
