use super::super::*;
use std::collections::BTreeSet;

#[test]
fn first_packet_never_publishes_a_detent_and_later_packets_use_one_slot() {
    let mut state = BridgeState::new(100_000_000);
    state
        .accept_line(
            super::packet(1, 1, "X", "X1", true, false, true, 0).as_bytes(),
            20_000_000,
        )
        .unwrap();
    assert!(state.snapshot.connected);
    assert_eq!(state.snapshot.latest_detent, 0);
    state
        .accept_line(
            super::packet(2, -1, "X", "X1", true, false, true, 0).as_bytes(),
            40_000_000,
        )
        .unwrap();
    assert_eq!(state.snapshot.latest_detent, -1);
}

#[test]
fn sequence_delta_boundaries_wrap_and_gaps_are_exact() {
    for (previous, current, accepted, dropped) in [
        (7, 7, false, 0),
        (7, 8, true, 0),
        (7, 9, true, 1),
        (0, i32::MAX as u32, true, i32::MAX as u32 - 1),
        (0, i32::MAX as u32 + 1, false, 0),
        (u32::MAX, 0, true, 0),
    ] {
        let mut state = BridgeState::new(100_000_000);
        state
            .accept_line(
                super::packet(previous, 0, "X", "X1", false, false, true, 0).as_bytes(),
                0,
            )
            .unwrap();
        let result = state.accept_line(
            super::packet(current, 1, "X", "X1", true, false, true, 0).as_bytes(),
            20_000_000,
        );
        assert_eq!(result.is_ok(), accepted, "{previous} -> {current}");
        if accepted {
            assert!(state.snapshot.connected);
            assert_eq!(state.snapshot.dropped_packets, dropped);
            assert_eq!(state.snapshot.latest_detent, 1);
        } else {
            assert_eq!(result, Err(ProtocolError::RepeatedOrReversedSequence));
            assert!(state.snapshot.serial_fault);
            assert!(!state.snapshot.connected);
            assert_eq!(state.snapshot.latest_detent, 0);
        }
    }
}

#[test]
fn every_protocol_error_code_fails_closed_and_is_counted_once() {
    let direct_cases: [(&[u8], ProtocolError); 10] = [
        (b"P3,1", ProtocolError::WrongFieldCount),
        (b"P2,1,20,0,0,0,0,X,X1,0,0,1", ProtocolError::WrongMarker),
        (
            b"P3,no,20,0,0,0,0,X,X1,0,0,1",
            ProtocolError::InvalidInteger,
        ),
        (b"P3,1,20,0,0,0,2,X,X1,0,0,1", ProtocolError::InvalidDetent),
        (b"P3,1,20,0,0,0,0,Q,X1,0,0,1", ProtocolError::InvalidAxis),
        (
            b"P3,1,20,0,0,0,0,X,X2,0,0,1",
            ProtocolError::InvalidMultiplier,
        ),
        (b"P3,1,20,0,0,0,0,X,X1,2,0,1", ProtocolError::InvalidBoolean),
        (
            b"BOOT,P3,WRONG,MONITOR_ONLY",
            ProtocolError::UnexpectedBootMarker,
        ),
        (&[0xff], ProtocolError::NonAscii),
        (
            &[b'A'; MAX_SERIAL_LINE_BYTES + 1],
            ProtocolError::OverlongLine,
        ),
    ];
    let mut observed = Vec::new();
    for (line, expected) in direct_cases {
        let mut state = BridgeState::new(100_000_000);
        assert_eq!(state.accept_line(line, 0), Err(expected));
        assert!(!state.snapshot.connected);
        assert!(state.snapshot.serial_fault);
        assert!(state.snapshot.estop_pressed);
        assert_eq!(state.snapshot.latest_detent, 0);
        assert_eq!(state.snapshot.protocol_errors, 1);
        observed.push(expected);
    }

    let mut repeated = BridgeState::new(100_000_000);
    repeated.accept_line(super::IDLE.as_bytes(), 0).unwrap();
    assert_eq!(
        repeated.accept_line(super::IDLE.as_bytes(), 20_000_000),
        Err(ProtocolError::RepeatedOrReversedSequence)
    );
    assert_eq!(repeated.snapshot.protocol_errors, 1);
    observed.push(ProtocolError::RepeatedOrReversedSequence);

    assert_eq!(observed.len(), ProtocolError::ALL.len());
    assert_eq!(
        observed.into_iter().collect::<BTreeSet<_>>(),
        ProtocolError::ALL.into_iter().collect::<BTreeSet<_>>()
    );
}

#[test]
fn timeout_boundary_reversed_clock_and_packet_age_are_exact() {
    let mut state = BridgeState::new(100_000_000);
    state
        .accept_line(super::IDLE.as_bytes(), 20_000_000)
        .unwrap();
    assert_eq!(state.packet_age_ms(10_000_000), 0.0);
    assert_eq!(state.packet_age_ms(120_000_000), 100.0);
    assert!(!state.check_timeout(120_000_000));
    assert!(state.check_timeout(120_000_001));
    assert_eq!(state.snapshot.timeouts, 1);
    assert_eq!(state.packet_age_ms(120_000_001), -1.0);
    assert!(!state.check_timeout(u64::MAX));
    assert_eq!(state.snapshot.timeouts, 1);
}

#[test]
fn every_reset_path_requires_a_fresh_non_commanding_baseline() {
    enum Reset {
        Serial,
        Protocol,
        Timeout,
        Boot,
    }
    for reset in [Reset::Serial, Reset::Protocol, Reset::Timeout, Reset::Boot] {
        let mut state = BridgeState::new(100_000_000);
        state.accept_line(super::IDLE.as_bytes(), 0).unwrap();
        match reset {
            Reset::Serial => state.note_serial_fault(),
            Reset::Protocol => state.note_protocol_error(),
            Reset::Timeout => assert!(state.check_timeout(100_000_001)),
            Reset::Boot => state.accept_line(BOOT_MARKER.as_bytes(), 1).unwrap(),
        }
        state
            .accept_line(
                super::packet(50, 1, "X", "X1", true, false, true, 0).as_bytes(),
                120_000_000,
            )
            .unwrap();
        assert!(state.snapshot.connected);
        assert_eq!(state.snapshot.latest_detent, 0);
    }
}

#[test]
fn quadrature_fault_latches_until_the_exact_boot_marker() {
    let mut state = BridgeState::new(100_000_000);
    state
        .accept_line(
            super::packet(1, 0, "X", "X1", true, false, true, 3).as_bytes(),
            0,
        )
        .unwrap();
    state
        .accept_line(
            super::packet(2, 1, "X", "X1", true, false, true, 4).as_bytes(),
            20_000_000,
        )
        .unwrap();
    assert!(state.snapshot.quadrature_fault);
    assert!(!state.snapshot.link_healthy);
    assert_eq!(state.snapshot.latest_detent, 0);

    state.note_serial_fault();
    state
        .accept_line(
            super::packet(3, -1, "X", "X1", true, false, true, 4).as_bytes(),
            40_000_000,
        )
        .unwrap();
    assert!(state.snapshot.quadrature_fault);
    assert_eq!(state.snapshot.latest_detent, 0);

    state
        .accept_line(BOOT_MARKER.as_bytes(), 60_000_000)
        .unwrap();
    assert!(!state.snapshot.quadrature_fault);
    assert!(state.snapshot.serial_fault);
    assert!(state.snapshot.estop_pressed);
}

#[test]
fn diagnostic_counters_wrap_without_panicking_and_boot_preserves_them() {
    let mut state = BridgeState::new(0);
    state.snapshot.protocol_errors = u32::MAX;
    state.note_protocol_error();
    assert_eq!(state.snapshot.protocol_errors, 0);

    state.snapshot.dropped_packets = u32::MAX;
    state.snapshot.timeouts = u32::MAX;
    state.accept_line(BOOT_MARKER.as_bytes(), 0).unwrap();
    assert_eq!(state.snapshot.protocol_errors, 0);
    assert_eq!(state.snapshot.dropped_packets, u32::MAX);
    assert_eq!(state.snapshot.timeouts, u32::MAX);
}
