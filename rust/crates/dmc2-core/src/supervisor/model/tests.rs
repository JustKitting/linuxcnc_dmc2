use super::*;

const fn bit(mask: u16, index: u32) -> bool {
    mask & (1_u16 << index) != 0
}

#[test]
fn every_machine_readiness_boolean_state_has_one_exact_result() {
    let mut states = 0_u32;
    for mask in 0_u16..(1_u16 << 12) {
        let machine_on = bit(mask, 0);
        let estopped = bit(mask, 1);
        let manual_mode = bit(mask, 2);
        let joint_mode = bit(mask, 3);
        let teleop_mode = bit(mask, 4);
        let interp_idle = bit(mask, 5);
        let homed = [bit(mask, 6), bit(mask, 7), bit(mask, 8)];
        let homing = [bit(mask, 9), bit(mask, 10), bit(mask, 11)];
        let machine = MachineSnapshot {
            machine_on,
            estopped,
            manual_mode,
            joint_mode,
            teleop_mode,
            interp_idle,
            homed,
            homing,
            axis_stopped: [bit(mask, 1), bit(mask, 4), bit(mask, 8)],
        };
        let all_homed = homed.into_iter().all(|value| value);
        let any_homing = homing.into_iter().any(|value| value);
        let expected = machine_on
            && !estopped
            && manual_mode
            && (if all_homed { teleop_mode } else { joint_mode })
            && interp_idle
            && !any_homing;

        assert_eq!(machine.all_homed(), all_homed, "mask=0x{mask:03x}");
        assert_eq!(machine.any_homing(), any_homing, "mask=0x{mask:03x}");
        assert_eq!(
            machine.ready_for_pendant_jog(),
            expected,
            "mask=0x{mask:03x}"
        );
        states += 1;
    }
    assert_eq!(states, 4_096);
}
