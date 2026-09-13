use super::*;

fn released() -> Inputs {
    Inputs {
        circuits: Some(Circuits {
            contact_closed: true,
            overtravel_closed: true,
        }),
        manual_idle_stationary: true,
        program_active: false,
        clear_setter: false,
        clear_fault: false,
    }
}

fn trip(setter: &mut ToolSetter) {
    let mut input = released();
    input.circuits.as_mut().unwrap().overtravel_closed = false;
    assert!(setter.update(input).latched);
}

#[test]
fn normally_closed_contact_and_overtravel_are_independent() {
    for contact_closed in [false, true] {
        for overtravel_closed in [false, true] {
            let mut setter = ToolSetter::default();
            let mut input = released();
            input.circuits = Some(Circuits {
                contact_closed,
                overtravel_closed,
            });
            let out = setter.update(input);
            assert_eq!(out.contact, !contact_closed);
            assert_eq!(out.overtravel, !overtravel_closed);
            assert_eq!(out.latched, !overtravel_closed);
        }
    }
}

#[test]
fn release_does_not_resume_program_and_manual_withdrawal_has_no_abort_request() {
    let mut setter = ToolSetter::default();
    trip(&mut setter);
    let mut input = released();
    input.program_active = true;
    input.manual_idle_stationary = false;
    let out = setter.update(input);
    assert!(out.feed_inhibit && out.abort_program);
    assert_eq!(out.status, Status::NeedsManualIdle);
    let out = setter.update(released());
    assert!(out.feed_inhibit && !out.abort_program);
    assert_eq!(out.status, Status::NeedsAcknowledgement);
}

#[test]
fn rejected_clear_is_not_applied_later_while_held() {
    for unavailable in [false, true] {
        let mut setter = ToolSetter::default();
        trip(&mut setter);
        let mut input = released();
        input.clear_setter = true;
        if unavailable {
            input.circuits = None;
        } else {
            input.manual_idle_stationary = false;
        }
        assert!(setter.update(input).latched);
        let mut input = released();
        input.clear_setter = true;
        assert!(setter.update(input).latched);
        setter.update(released());
        let mut input = released();
        input.clear_setter = true;
        assert!(!setter.update(input).latched);
    }
}

#[test]
fn neither_open_circuit_allows_clear_and_each_ui_control_has_its_own_edge() {
    let mut setter = ToolSetter::default();
    trip(&mut setter);
    for circuits in [
        Circuits {
            contact_closed: false,
            overtravel_closed: true,
        },
        Circuits {
            contact_closed: true,
            overtravel_closed: false,
        },
    ] {
        setter.update(released());
        let mut input = released();
        input.circuits = Some(circuits);
        input.clear_setter = true;
        assert!(setter.update(input).latched);
    }
    let mut input = released();
    input.clear_setter = true;
    input.clear_fault = true;
    assert!(!setter.update(input).latched);
}

#[test]
fn missing_sample_keeps_feed_inhibited_but_never_rejects_global_clear() {
    let mut setter = ToolSetter::default();
    let mut input = released();
    input.circuits = None;
    let out = setter.update(input);
    assert!(out.feed_inhibit && !out.latched && !out.contact);
    setter.update(released());
    let mut input = released();
    input.circuits = None;
    let out = setter.update(input);
    assert!(!out.latched && !out.contact && out.feed_inhibit);
    assert_eq!(out.status, Status::Unavailable);
    trip(&mut setter);
    let mut input = released();
    input.circuits = None;
    input.clear_fault = true;
    let out = setter.update(input);
    assert!(!out.latched);
    assert!(out.feed_inhibit);
    assert_eq!(out.status, Status::Unavailable);
}

#[test]
fn direct_clear_is_independent_of_a_held_canonical_reset_and_stale_task() {
    let mut setter = ToolSetter::default();
    trip(&mut setter);
    let mut input = released();
    input.clear_fault = true;
    setter.update(input);
    let mut input = released();
    input.clear_fault = true;
    input.circuits.as_mut().unwrap().overtravel_closed = false;
    assert!(setter.update(input).latched);
    setter.clear_fault();
    let mut input = released();
    input.clear_fault = true;
    input.manual_idle_stationary = false;
    let out = setter.update(input);
    assert!(!out.latched);
    assert_eq!(out.status, Status::Ready);
}
