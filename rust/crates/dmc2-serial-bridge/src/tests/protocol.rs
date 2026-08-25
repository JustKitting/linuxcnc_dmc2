use super::super::*;

fn replace_field(line: &str, index: usize, replacement: &str) -> String {
    let mut fields = line.split(',').collect::<Vec<_>>();
    fields[index] = replacement;
    fields.join(",")
}

#[test]
fn every_axis_multiplier_and_boolean_wire_code_maps_exactly() {
    let axes = [
        ("X", AxisCode::X),
        ("Y", AxisCode::Y),
        ("Z", AxisCode::Z),
        ("4", AxisCode::Axis4),
        ("5", AxisCode::Axis5),
        ("N", AxisCode::Off),
        ("I", AxisCode::Invalid),
    ];
    let multipliers = [
        ("X1", MultiplierCode::X1),
        ("X10", MultiplierCode::X10),
        ("X100", MultiplierCode::X100),
        ("N", MultiplierCode::Off),
        ("I", MultiplierCode::Invalid),
    ];
    for (axis_token, axis) in axes {
        for (multiplier_token, multiplier) in multipliers {
            for bits in 0_u8..8 {
                let line = format!(
                    "P3,1,20,0,0,0,0,{axis_token},{multiplier_token},{},{},{}",
                    bits & 1,
                    (bits >> 1) & 1,
                    (bits >> 2) & 1,
                );
                let parsed = parse_packet(&line).unwrap();
                assert_eq!(parsed.axis, axis);
                assert_eq!(parsed.multiplier, multiplier);
                assert_eq!(parsed.deadman_held, bits & 1 != 0);
                assert_eq!(parsed.estop_pressed, bits & 2 != 0);
                assert_eq!(parsed.selector_valid, bits & 4 != 0);
            }
        }
    }
}

#[test]
fn every_integer_field_accepts_its_exact_wire_boundaries() {
    let packet =
        parse_packet("P3,4294967295,0,-2147483648,2147483647,4294967295,-1,I,I,0,1,0").unwrap();
    assert_eq!(packet.sequence, u32::MAX);
    assert_eq!(packet.milliseconds, 0);
    assert_eq!(packet.detent_count, i32::MIN);
    assert_eq!(packet.transition_count, i32::MAX);
    assert_eq!(packet.quadrature_errors, u32::MAX);
    assert_eq!(packet.latest_detent, -1);

    for index in [1, 2, 5] {
        for invalid in ["-1", "+1", "00", "01", "4294967296", " 1", "1 "] {
            assert_eq!(
                parse_packet(&replace_field(super::IDLE, index, invalid)),
                Err(ProtocolError::InvalidInteger),
                "field {index}, token {invalid:?}"
            );
        }
    }
    for index in [3, 4] {
        for invalid in [
            "+1",
            "-0",
            "00",
            "01",
            "-01",
            "2147483648",
            "-2147483649",
            " 1",
            "1 ",
        ] {
            assert_eq!(
                parse_packet(&replace_field(super::IDLE, index, invalid)),
                Err(ProtocolError::InvalidInteger),
                "field {index}, token {invalid:?}"
            );
        }
    }
}

#[test]
fn all_parser_error_codes_have_an_exact_trigger() {
    let cases = [
        ("P3,1", ProtocolError::WrongFieldCount),
        ("P2,1,20,0,0,0,0,X,X1,0,0,1", ProtocolError::WrongMarker),
        ("P3,no,20,0,0,0,0,X,X1,0,0,1", ProtocolError::InvalidInteger),
        ("P3,1,20,0,0,0,2,X,X1,0,0,1", ProtocolError::InvalidDetent),
        ("P3,1,20,0,0,0,0,Q,X1,0,0,1", ProtocolError::InvalidAxis),
        (
            "P3,1,20,0,0,0,0,X,X2,0,0,1",
            ProtocolError::InvalidMultiplier,
        ),
        ("P3,1,20,0,0,0,0,X,X1,2,0,1", ProtocolError::InvalidBoolean),
    ];
    for (line, expected) in cases {
        assert_eq!(parse_packet(line), Err(expected), "{line}");
    }
}

#[test]
fn lengths_128_and_129_and_non_ascii_are_distinguished_exactly() {
    assert_eq!(
        parse_packet(&"A".repeat(128)),
        Err(ProtocolError::WrongFieldCount)
    );
    assert_eq!(
        parse_packet(&"A".repeat(129)),
        Err(ProtocolError::OverlongLine)
    );
    assert_eq!(parse_packet("é"), Err(ProtocolError::NonAscii));
}

#[test]
fn whitespace_and_control_bytes_can_never_be_trimmed_into_a_valid_packet() {
    for altered in [
        format!(" {}", super::IDLE),
        format!("{} ", super::IDLE),
        format!("\t{}", super::IDLE),
        format!("{}\r", super::IDLE),
        format!("{}\n", super::IDLE),
    ] {
        assert!(parse_packet(&altered).is_err(), "accepted {altered:?}");
    }
}

#[test]
fn every_possible_raw_byte_is_preserved_or_rejected_without_aliasing() {
    let valid = super::IDLE.as_bytes();
    for value in u8::MIN..=u8::MAX {
        let mut replaced = valid.to_vec();
        replaced[0] = value;
        let mut state = BridgeState::new(100_000_000);
        let result = state.accept_line(&replaced, 0);
        if value == b'P' {
            assert_eq!(result, Ok(()));
            assert!(state.snapshot.connected);
        } else {
            assert!(result.is_err(), "byte {value:#04x} aliased to P");
            assert!(!state.snapshot.connected);
            assert_eq!(state.snapshot.protocol_errors, 1);
        }
    }

    for value in u8::MIN..=u8::MAX {
        let mut appended = valid.to_vec();
        appended.push(value);
        let mut state = BridgeState::new(100_000_000);
        assert!(
            state.accept_line(&appended, 0).is_err(),
            "appended byte {value:#04x} was ignored"
        );
        assert_eq!(state.snapshot.protocol_errors, 1);
    }
}

#[test]
fn every_byte_at_every_position_has_the_exact_canonical_acceptance_set() {
    let canonical = super::IDLE.as_bytes();
    let allowed = |position: usize, value: u8| match position {
        0 => value == b'P',
        1 => value == b'3',
        2 | 4 | 7 | 9 | 11 | 13 | 15 | 17 | 20 | 22 | 24 => value == b',',
        3 | 6 | 8 | 10 | 12 => value.is_ascii_digit(),
        5 => matches!(value, b'1'..=b'9'),
        14 | 21 | 23 | 25 => matches!(value, b'0' | b'1'),
        16 => matches!(value, b'X' | b'Y' | b'Z' | b'4' | b'5' | b'N' | b'I'),
        18 => value == b'X',
        19 => value == b'1',
        _ => panic!("unaccounted canonical byte position {position}"),
    };

    assert_eq!(canonical.len(), 26);
    for position in 0..canonical.len() {
        for value in u8::MIN..=u8::MAX {
            let mut candidate = canonical.to_vec();
            candidate[position] = value;
            let accepted = core::str::from_utf8(&candidate)
                .ok()
                .and_then(|line| parse_packet(line).ok())
                .is_some();
            assert_eq!(
                accepted,
                allowed(position, value),
                "position {position}, byte {value:#04x}, candidate {candidate:?}"
            );
        }
    }
}
