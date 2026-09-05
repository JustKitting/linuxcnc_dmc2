use super::*;

const TIMEOUT_NS: u64 = 100_000_000;

fn packet(sequence: u32, quadrature_errors: u32) -> Packet {
    Packet {
        sequence,
        quadrature_errors,
        milliseconds: sequence * 20,
        detent_count: 0,
        transition_count: 0,
        latest_detent: 1,
        axis: AxisCode::X,
        multiplier: MultiplierCode::X1,
        deadman_held: false,
        estop_pressed: false,
        selector_valid: true,
    }
}

fn latched() -> BridgeState {
    let mut state = BridgeState::new(TIMEOUT_NS);
    state.accept(packet(1, 0), 0).unwrap();
    state.accept(packet(2, 1), 20_000_000).unwrap();
    assert!(state.snapshot.quadrature_fault);
    state
}

#[test]
fn healthy_packets_do_not_automatically_clear_decoder_fault() {
    let mut state = latched();
    for sequence in 3..20 {
        state
            .accept(packet(sequence, 1), u64::from(sequence) * 20_000_000)
            .unwrap();
        assert!(state.snapshot.quadrature_fault);
        assert_eq!(state.snapshot.latest_detent, 0);
    }
}

#[test]
fn explicit_reset_acknowledges_a_fresh_stable_packet_without_its_detent() {
    let mut state = latched();
    state.observe_fault_reset(7, 25_000_000);
    assert!(state.snapshot.quadrature_fault);
    state.accept(packet(3, 1), 40_000_000).unwrap();
    assert_eq!(state.snapshot.fault_reset_ack, 7);
    assert!(!state.snapshot.quadrature_fault);
    assert!(state.snapshot.current_fault.is_none());
    assert_eq!(state.snapshot.latest_detent, 0);
    state.accept(packet(4, 2), 60_000_000).unwrap();
    state.observe_fault_reset(7, 65_000_000);
    state.accept(packet(5, 2), 80_000_000).unwrap();
    assert!(
        state.snapshot.quadrature_fault,
        "a held request must not retry"
    );
}

#[test]
fn changed_counter_deadman_or_estop_does_not_acknowledge_reset() {
    for rejected in [
        packet(3, 2),
        Packet {
            deadman_held: true,
            ..packet(3, 1)
        },
        Packet {
            estop_pressed: true,
            ..packet(3, 1)
        },
    ] {
        let mut state = latched();
        state.observe_fault_reset(7, 25_000_000);
        state.accept(rejected, 40_000_000).unwrap();
        assert!(state.snapshot.quadrature_fault);
        assert_eq!(state.snapshot.fault_reset_ack, 0);
        state
            .accept(packet(4, rejected.quadrature_errors), 60_000_000)
            .unwrap();
        assert!(
            state.snapshot.quadrature_fault,
            "a rejected request must not retry"
        );
    }
}

#[test]
fn cancelled_expired_or_interrupted_requests_cannot_acknowledge_later() {
    for interruption in 0..5 {
        let mut state = latched();
        state.observe_fault_reset(7, 25_000_000);
        match interruption {
            0 => state.observe_fault_reset(0, 30_000_000),
            1 => {
                assert!(state.check_timeout(200_000_000));
            }
            2 => state.note_protocol_error(ProtocolError::NonAscii, Some(1)),
            3 => state.reset_for_boot(),
            4 => state.note_serial_read_failure(Some(5), None),
            _ => unreachable!(),
        }
        state.accept(packet(3, 1), 210_000_000).unwrap();
        assert_eq!(state.snapshot.fault_reset_ack, 0);
    }
    let mut state = latched();
    state.observe_fault_reset(7, 25_000_000);
    state.accept(packet(3, 1), 200_000_000).unwrap();
    assert!(state.snapshot.quadrature_fault);
    assert_eq!(state.snapshot.fault_reset_ack, 0);
}
