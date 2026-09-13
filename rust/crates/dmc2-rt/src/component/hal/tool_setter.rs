//! Normalize the two NC inputs before motion; keep recovery outside jog gates.
use super::super::StartupFailure;
use super::Pins as ControllerPins;
use crate::component::state::CachedTaskSnapshot;
use core::{
    ffi::{c_long, c_void},
    mem, ptr,
};
use dmc2_core::{
    tool_setter::{Circuits, Inputs, Status, ToolSetter},
    Freshness, TASK_HEARTBEAT_TIMEOUT_NS,
};
use dmc2_hal_sys as hal;

hal::realtime_hal_pin_catalog! {
    pub(super) struct Pins;
    pub(super) fn register_pins;
    pins {
        contact_closed: bit in => "dmc2-tool-setter.contact-closed";
        overtravel_closed: bit in => "dmc2-tool-setter.overtravel-closed";
        clear: bit in => "dmc2-tool-setter.clear";
        contact: bit out => "dmc2-tool-setter.contact";
        overtravel: bit out => "dmc2-tool-setter.overtravel";
        latched: bit out => "dmc2-tool-setter.latched";
        feed_inhibit: bit out => "dmc2-tool-setter.feed-inhibit";
        abort_program: bit out => "dmc2-tool-setter.abort-program";
        unavailable: bit out => "dmc2-tool-setter.unavailable";
        ready: bit out => "dmc2-tool-setter.ready";
        contact_open: bit out => "dmc2-tool-setter.contact-open";
        overtravel_open: bit out => "dmc2-tool-setter.overtravel-open";
        needs_manual_idle: bit out => "dmc2-tool-setter.needs-manual-idle";
        needs_acknowledgement: bit out => "dmc2-tool-setter.needs-acknowledgement";
    }
    numbered {}
}

struct State {
    pins: *mut Pins,
    controller: *mut ControllerPins,
    policy: ToolSetter,
    task_freshness: Freshness,
    task: CachedTaskSnapshot,
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
        ptr::write_volatile((*pins).feed_inhibit, true);
        ptr::write_volatile((*pins).unavailable, true);
        ptr::write(
            state,
            State {
                pins,
                controller,
                policy: ToolSetter::default(),
                task_freshness: Freshness::new(),
                task: CachedTaskSnapshot::safe(),
            },
        );
        hal::HalCall::ExportFunct.classify(hal::hal_export_funct(
            c"dmc2-tool-setter.update".as_ptr(),
            Some(update),
            state.cast(),
            0,
            0,
            component,
        ))?;
    }
    Ok(())
}

unsafe extern "C" fn update(arg: *mut c_void, period: c_long) {
    if period <= 0 {
        return;
    }
    let state = unsafe { &mut *arg.cast::<State>() };
    let pins = unsafe { &*state.pins };
    let c = unsafe { &*state.controller };
    unsafe {
        super::transport::refresh_task_snapshot(c, &mut state.task);
        let task_connected = state.task.connected && !state.task.fault;
        if task_connected {
            state
                .task_freshness
                .update(state.task.heartbeat, period as u64);
        } else {
            state.task_freshness.elapse(period as u64);
        }
        let task_valid = task_connected && state.task_freshness.is_fresh(TASK_HEARTBEAT_TIMEOUT_NS);
        // Mesa's existing transport/watchdog policy owns communication failure.
        // A discarded read must neither create a physical event nor clear one.
        let mesa_valid = ptr::read_volatile(c.servo_thread_ready)
            && !ptr::read_volatile(c.mesa_packet_error)
            && !ptr::read_volatile(c.mesa_packet_error_exceeded)
            && !ptr::read_volatile(c.mesa_watchdog_has_bit);
        let interp_idle = state.task.machine.interp_idle;
        let coord_mode = ptr::read_volatile(c.motion_coord_mode);
        let out = state.policy.update(Inputs {
            circuits: mesa_valid.then(|| Circuits {
                contact_closed: ptr::read_volatile(pins.contact_closed),
                overtravel_closed: ptr::read_volatile(pins.overtravel_closed),
            }),
            manual_idle_stationary: task_valid
                && state.task.machine.manual_mode
                && interp_idle
                && !coord_mode
                && ptr::read_volatile(c.motion_in_position)
                && !ptr::read_volatile(c.motion_jog_active)
                && !state.task.machine.homing.iter().any(|v| *v)
                && state.task.machine.axis_stopped.iter().all(|v| *v)
                && c.joint_in_position.iter().all(|p| ptr::read_volatile(*p)),
            program_active: !interp_idle || coord_mode,
            clear_setter: ptr::read_volatile(pins.clear),
            clear_fault: ptr::read_volatile(c.linuxcnc_estop_reset_request),
        });
        for (pin, value) in [
            (pins.contact, out.contact),
            (pins.overtravel, out.overtravel),
            (pins.latched, out.latched),
            (pins.feed_inhibit, out.feed_inhibit),
            (pins.abort_program, out.abort_program),
            (pins.unavailable, out.status == Status::Unavailable),
            (pins.ready, out.status == Status::Ready),
            (pins.contact_open, out.status == Status::ContactOpen),
            (pins.overtravel_open, out.status == Status::OvertravelOpen),
            (
                pins.needs_manual_idle,
                out.status == Status::NeedsManualIdle,
            ),
            (
                pins.needs_acknowledgement,
                out.status == Status::NeedsAcknowledgement,
            ),
        ] {
            ptr::write_volatile(pin, value);
        }
    }
}
