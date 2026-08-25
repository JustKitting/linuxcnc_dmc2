use super::*;

const VALID: &[u8] = b"P3,1,20,0,0,0,0,X,X1,0,0,1";

fn feed(assembler: &mut LineAssembler, state: &mut BridgeState, bytes: &[u8]) -> Vec<LineEvent> {
    bytes
        .iter()
        .copied()
        .map(|byte| assembler.consume(byte, state, 20_000_000))
        .collect()
}

fn non_pending(events: &[LineEvent]) -> Vec<LineEvent> {
    events
        .iter()
        .copied()
        .filter(|event| *event != LineEvent::Pending)
        .collect()
}

#[test]
fn lf_and_exact_terminal_crlf_are_the_only_accepted_terminators() {
    for suffix in [b"\n".as_slice(), b"\r\n".as_slice()] {
        let mut assembler = LineAssembler::new();
        let mut state = BridgeState::new(100_000_000);
        let mut frame = VALID.to_vec();
        frame.extend_from_slice(suffix);
        let events = feed(&mut assembler, &mut state, &frame);
        assert_eq!(non_pending(&events), [LineEvent::Accepted]);
        assert!(state.snapshot.connected);
        assert_eq!(state.snapshot.protocol_errors, 0);
    }

    for altered in [
        b"\rP3,1,20,0,0,0,0,X,X1,0,0,1\n".as_slice(),
        b"P3,1,20,0,0,0,0,X,X1,0,0,\r1\n".as_slice(),
        b"P3,1,20,0,0,0,0,X,X1,0,0,1\r\r\n".as_slice(),
    ] {
        let mut assembler = LineAssembler::new();
        let mut state = BridgeState::new(100_000_000);
        let events = feed(&mut assembler, &mut state, altered);
        assert!(matches!(
            non_pending(&events).as_slice(),
            [LineEvent::Rejected(_)]
        ));
        assert!(state.snapshot.serial_fault);
        assert_eq!(state.snapshot.protocol_errors, 1);
    }
}

#[test]
fn every_possible_terminal_byte_has_one_deterministic_classification() {
    for value in u8::MIN..=u8::MAX {
        let mut assembler = LineAssembler::new();
        let mut state = BridgeState::new(100_000_000);
        let prefix = feed(&mut assembler, &mut state, VALID);
        assert!(prefix.iter().all(|event| *event == LineEvent::Pending));

        let event = assembler.consume(value, &mut state, 20_000_000);
        match value {
            b'\n' => {
                assert_eq!(event, LineEvent::Accepted);
                assert!(state.snapshot.connected);
                assert_eq!(state.snapshot.protocol_errors, 0);
            }
            b'\r' => {
                assert_eq!(event, LineEvent::Pending);
                assert_eq!(
                    assembler.consume(b'\n', &mut state, 20_000_000),
                    LineEvent::Accepted
                );
                assert!(state.snapshot.connected);
                assert_eq!(state.snapshot.protocol_errors, 0);
            }
            _ => {
                assert_eq!(event, LineEvent::Pending);
                assert!(matches!(
                    assembler.consume(b'\n', &mut state, 20_000_000),
                    LineEvent::Rejected(_)
                ));
                assert!(!state.snapshot.connected);
                assert_eq!(state.snapshot.protocol_errors, 1, "byte {value:#04x}");
            }
        }
    }
}

#[test]
fn payload_lengths_127_128_and_129_have_exact_boundaries() {
    for length in [127, 128] {
        let mut assembler = LineAssembler::new();
        let mut state = BridgeState::new(100_000_000);
        let mut bytes = vec![b'A'; length];
        bytes.extend_from_slice(b"\r\n");
        let events = non_pending(&feed(&mut assembler, &mut state, &bytes));
        assert_eq!(
            events,
            [LineEvent::Rejected(ProtocolError::WrongFieldCount)]
        );
        assert_eq!(state.snapshot.protocol_errors, 1);
    }

    let mut assembler = LineAssembler::new();
    let mut state = BridgeState::new(100_000_000);
    let events = feed(&mut assembler, &mut state, &[b'A'; 129]);
    assert_eq!(
        non_pending(&events),
        [LineEvent::Rejected(ProtocolError::OverlongLine)]
    );
    assert!(state.snapshot.serial_fault);
    assert_eq!(state.snapshot.protocol_errors, 1);
    assert_eq!(
        assembler.consume(b'\n', &mut state, 20_000_000),
        LineEvent::DiscardedAfterOverlong
    );
    assert_eq!(state.snapshot.protocol_errors, 1);
}

#[test]
fn an_arbitrarily_long_frame_faults_once_stays_bounded_and_recovers_at_lf() {
    let mut assembler = LineAssembler::new();
    let mut state = BridgeState::new(100_000_000);
    let events = feed(&mut assembler, &mut state, &vec![b'X'; 100_000]);
    assert_eq!(
        non_pending(&events),
        [LineEvent::Rejected(ProtocolError::OverlongLine)]
    );
    assert_eq!(state.snapshot.protocol_errors, 1);
    assert!(state.snapshot.serial_fault);

    assert_eq!(
        assembler.consume(b'\n', &mut state, 30_000_000),
        LineEvent::DiscardedAfterOverlong
    );
    assert_eq!(state.snapshot.protocol_errors, 1);

    let mut valid = VALID.to_vec();
    valid.push(b'\n');
    assert_eq!(
        non_pending(&feed(&mut assembler, &mut state, &valid)),
        [LineEvent::Accepted]
    );
    assert!(state.snapshot.connected);
    assert_eq!(state.snapshot.latest_detent, 0);
    assert_eq!(state.snapshot.protocol_errors, 1);
}

#[test]
fn empty_frames_are_explicit_and_do_not_change_state() {
    let mut assembler = LineAssembler::new();
    let mut state = BridgeState::new(100_000_000);
    let original = state;
    assert_eq!(assembler.consume(b'\n', &mut state, 0), LineEvent::Empty);
    assert_eq!(
        feed(&mut assembler, &mut state, b"\r\n"),
        [LineEvent::Pending, LineEvent::Empty,]
    );
    assert_eq!(state, original);
    assert!(LineEvent::Empty.requires_publish());
    assert!(!LineEvent::Pending.requires_publish());
}

#[test]
fn every_input_chunk_split_produces_the_same_frame_and_snapshot() {
    let mut wire = VALID.to_vec();
    wire.extend_from_slice(b"\r\n");
    let expected = {
        let mut assembler = LineAssembler::new();
        let mut state = BridgeState::new(100_000_000);
        let events = feed(&mut assembler, &mut state, &wire);
        (non_pending(&events), state)
    };

    for split in 0..=wire.len() {
        let mut assembler = LineAssembler::new();
        let mut state = BridgeState::new(100_000_000);
        let mut events = feed(&mut assembler, &mut state, &wire[..split]);
        events.extend(feed(&mut assembler, &mut state, &wire[split..]));
        assert_eq!((non_pending(&events), state), expected, "split {split}");
    }
}
