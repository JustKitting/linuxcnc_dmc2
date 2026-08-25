use super::*;

const IDLE: &str = "P3,1,20,0,0,0,0,X,X1,0,0,1";

#[test]
fn parses_exact_p3_packet() {
    let packet = parse_packet("P3,2,40,-1,4,0,1,Z,X100,1,0,1").unwrap();
    assert_eq!(packet.sequence, 2);
    assert_eq!(packet.detent_count, -1);
    assert_eq!(packet.axis, AxisCode::Z);
    assert_eq!(packet.multiplier, MultiplierCode::X100);
    assert!(packet.deadman_held);
    assert_eq!(packet.latest_detent, 1);
}

#[test]
fn first_packet_never_publishes_a_detent() {
    let mut state = BridgeState::new(100_000_000);
    state
        .accept_line(b"P3,1,20,1,4,0,1,X,X1,1,0,1", 20_000_000)
        .unwrap();
    assert!(state.snapshot.connected);
    assert_eq!(state.snapshot.latest_detent, 0);
    state
        .accept_line(b"P3,2,40,2,8,0,1,X,X1,1,0,1", 40_000_000)
        .unwrap();
    assert_eq!(state.snapshot.latest_detent, 1);
}

#[test]
fn sequence_gap_counts_loss_without_creating_a_queue() {
    let mut state = BridgeState::new(100_000_000);
    state.accept_line(IDLE.as_bytes(), 20_000_000).unwrap();
    state
        .accept_line(b"P3,1000001,40,9,9,0,-1,Y,X10,1,0,1", 40_000_000)
        .unwrap();
    assert_eq!(state.snapshot.dropped_packets, 999_999);
    assert_eq!(state.snapshot.latest_detent, -1);
}

#[test]
fn timeout_and_protocol_error_publish_safe_state() {
    let mut state = BridgeState::new(100_000_000);
    state.accept_line(IDLE.as_bytes(), 0).unwrap();
    assert!(!state.check_timeout(100_000_000));
    assert!(state.check_timeout(100_000_001));
    assert!(!state.snapshot.connected);
    assert!(state.snapshot.serial_fault);
    assert!(state.snapshot.estop_pressed);

    assert!(state.accept_line(b"P3,broken", 200_000_000).is_err());
    assert_eq!(state.snapshot.protocol_errors, 1);
    assert!(state.snapshot.estop_pressed);
}

#[test]
fn quadrature_error_latches_and_suppresses_detents() {
    let mut state = BridgeState::new(100_000_000);
    state.accept_line(IDLE.as_bytes(), 0).unwrap();
    state
        .accept_line(b"P3,2,20,1,4,1,1,X,X1,1,0,1", 20_000_000)
        .unwrap();
    assert!(state.snapshot.quadrature_fault);
    assert_eq!(state.snapshot.latest_detent, 0);
    assert!(!state.snapshot.link_healthy);
}
