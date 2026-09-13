//! Capture after motion-controller exports this cycle's joint feedback.
use super::super::StartupFailure;
use super::Pins as ControllerPins;
use core::{
    ffi::{c_long, c_void},
    mem, ptr,
};
use dmc2_hal_sys::{
    self as hal,
    probe_stream::{flag, Frame, Stream},
};

hal::realtime_hal_pin_catalog! {
    pub(super) struct Pins;
    pub(super) fn register_pins;
    pins {
        mode: bit in => "dmc2-probe.mode-request";
        record: bit in => "dmc2-probe.record-request";
        contact: bit in => "dmc2-probe.contact";
        selected_contact: bit in => "dmc2-probe.selected-contact";
        all_homed: bit in => "dmc2-probe.all-homed";
        selected: bit out => "dmc2-probe.manual-selected";
        jog_contact: bit out => "dmc2-probe.jog-contact";
        recording: bit out => "dmc2-probe.recording";
        dropped: u32 out => "dmc2-probe.dropped-samples";
        position: float[3] in => ["dmc2-probe.x", "dmc2-probe.y", "dmc2-probe.z"];
        command_position: float[3] in => ["dmc2-probe.command-x", "dmc2-probe.command-y", "dmc2-probe.command-z"];
        velocity: float[3] in => ["dmc2-probe.velocity-x", "dmc2-probe.velocity-y", "dmc2-probe.velocity-z"];
    }
    numbered {}
}

struct State {
    pins: *mut Pins,
    controller: *mut ControllerPins,
    stream: Stream,
    previous: Frame,
    elapsed_ns: u64,
    cycle: u32,
    dropped: u32,
    pending_off: bool,
    gap: bool,
    previous_selected_contact: bool,
}

pub(in crate::component) unsafe fn install(
    component: i32,
    controller: *mut ControllerPins,
) -> Result<(), StartupFailure> {
    let _ = Pins::PIN_COUNT;
    let pins = unsafe { hal::hal_malloc(mem::size_of::<Pins>() as c_long).cast::<Pins>() };
    let state = unsafe { hal::hal_malloc(mem::size_of::<State>() as c_long).cast::<State>() };
    if pins.is_null() || state.is_null() {
        return Err(StartupFailure::StateAllocation);
    }
    unsafe {
        ptr::write_bytes(pins, 0, 1);
        register_pins(pins, component)?;
    }
    let stream = unsafe { Stream::create(component) }.map_err(StartupFailure::ProbeStream)?;
    unsafe {
        ptr::write(
            state,
            State {
                pins,
                controller,
                stream,
                previous: Frame::default(),
                elapsed_ns: 0,
                cycle: 0,
                dropped: 0,
                pending_off: false,
                gap: false,
                previous_selected_contact: false,
            },
        );
    }
    for (name, function) in [
        (
            b"dmc2-probe.select\0".as_slice(),
            select as unsafe extern "C" fn(*mut c_void, c_long),
        ),
        (
            b"dmc2-probe.capture\0".as_slice(),
            capture as unsafe extern "C" fn(*mut c_void, c_long),
        ),
    ] {
        hal::HalCall::ExportFunct.classify(unsafe {
            hal::hal_export_funct(
                name.as_ptr().cast(),
                Some(function),
                state.cast(),
                1,
                0,
                component,
            )
        })?;
    }
    Ok(())
}

unsafe extern "C" fn select(arg: *mut c_void, _period: c_long) {
    let state = unsafe { &mut *arg.cast::<State>() };
    let pins = unsafe { &*state.pins };
    let controller = unsafe { &*state.controller };
    // P1 program selection remains independent. Manual Probe Mode must not
    // substitute IN1 for a tool-setter or another programmed probe operation.
    let selected = unsafe {
        ptr::read_volatile(pins.mode)
            && ptr::read_volatile(controller.manual_mode)
            && ptr::read_volatile(controller.interp_idle)
            && !ptr::read_volatile(controller.motion_coord_mode)
            && !controller
                .joint_homing
                .iter()
                .any(|p| ptr::read_volatile(*p))
    };
    unsafe {
        ptr::write_volatile(pins.selected, selected);
    }
}

unsafe extern "C" fn capture(arg: *mut c_void, period: c_long) {
    if period <= 0 {
        return;
    }
    let state = unsafe { &mut *arg.cast::<State>() };
    let pins = unsafe { &*state.pins };
    let c = unsafe { &*state.controller };
    state.elapsed_ns = state.elapsed_ns.saturating_add(period as u64);
    state.cycle = state.cycle.wrapping_add(1);
    let mut frame = Frame {
        seconds: state.elapsed_ns as f64 / 1e9,
        cycle: state.cycle,
        period_ns: period as u32,
        ..Frame::default()
    };
    let mode = unsafe { ptr::read_volatile(pins.mode) };
    let contact = unsafe { ptr::read_volatile(pins.contact) };
    let recording = mode && unsafe { ptr::read_volatile(pins.record) };
    unsafe {
        for (value, bit) in [
            (mode, flag::MODE),
            (ptr::read_volatile(pins.all_homed), flag::ALL_HOMED),
            (recording, flag::RECORD),
            (contact, flag::CONTACT),
            (
                mode && state.previous.has(flag::MODE)
                    && contact
                    && !state.previous.has(flag::CONTACT),
                flag::TOUCH,
            ),
            (ptr::read_volatile(c.deadman_held), flag::DEADMAN),
            (ptr::read_volatile(c.motion_enabled), flag::ENABLED),
            (ptr::read_volatile(c.manual_mode), flag::MANUAL),
            (ptr::read_volatile(c.motion_teleop_mode), flag::TELEOP),
            (ptr::read_volatile(c.motion_coord_mode), flag::COORD),
            (ptr::read_volatile(c.interp_idle), flag::IDLE),
            (
                ptr::read_volatile(c.mesa_packet_error)
                    || ptr::read_volatile(c.mesa_packet_error_exceeded)
                    || ptr::read_volatile(c.mesa_watchdog_has_bit),
                flag::TRANSPORT_BAD,
            ),
            (ptr::read_volatile(c.fault), flag::CONTROLLER_FAULT),
            (
                c.joint_homing.iter().any(|p| ptr::read_volatile(*p)),
                flag::HOMING,
            ),
            (ptr::read_volatile(pins.selected), flag::SELECTED),
            (ptr::read_volatile(c.pendant_mode_enabled), flag::PENDANT),
            (state.gap, flag::GAP),
        ] {
            if value {
                frame.flags |= bit;
            }
        }
        frame.phase = ptr::read_volatile(c.supervisor_phase);
        frame.axis = ptr::read_volatile(c.axis_code);
        frame.multiplier = ptr::read_volatile(c.multiplier_code);
        frame.detents = ptr::read_volatile(c.detent_count);
        for axis in 0..3 {
            frame.homed |= u32::from(ptr::read_volatile(c.joint_homed[axis])) << axis;
            frame.position[axis] = ptr::read_volatile(pins.position[axis]);
            frame.command_position[axis] = ptr::read_volatile(pins.command_position[axis]);
            frame.command_velocity[axis] = ptr::read_volatile(pins.velocity[axis]);
            frame.feedback_velocity[axis] =
                (frame.position[axis] - state.previous.position[axis]) / (period as f64 / 1e9);
        }
        if frame.valid_position() && state.previous.valid_position() {
            frame.flags |= flag::VELOCITY_VALID;
        }
        ptr::write_volatile(pins.recording, recording);
        // Retire a manual jog stopped by either selected contact, including
        // the tool setter outside XYZ Probe Mode. Recorder touches stay XYZ.
        let selected_contact = ptr::read_volatile(pins.selected_contact);
        ptr::write_volatile(
            pins.jog_contact,
            selected_contact
                && !state.previous_selected_contact
                && frame.has(flag::MANUAL)
                && frame.has(flag::IDLE)
                && !frame.has(flag::COORD)
                && !frame.has(flag::HOMING),
        );
        state.previous_selected_contact = selected_contact;
    }
    state.pending_off |= !mode && state.previous.has(flag::MODE);
    if mode || state.pending_off {
        if state.stream.write(frame) {
            state.pending_off = false;
            state.gap = false;
        } else {
            state.dropped = state.dropped.saturating_add(1);
            state.gap = true;
            unsafe {
                ptr::write_volatile(pins.dropped, state.dropped);
            }
        }
    }
    state.previous = frame;
}
