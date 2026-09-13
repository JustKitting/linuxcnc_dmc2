use core::ffi::{c_long, c_void};

use super::hal::{publish, runtime_inputs};
use super::state::ComponentState;

pub(super) unsafe extern "C" fn update_component(argument: *mut c_void, period: c_long) {
    if argument.is_null() || period <= 0 {
        return;
    }
    let state = unsafe { &mut *argument.cast::<ComponentState>() };
    let pins = unsafe { &*state.pins };
    let period_ns = period as u64;
    let advance_error = state.motion_commands.advance(period_ns).err();
    if unsafe { super::hal::manual_probe_contact(pins) } {
        state.runtime.observe_manual_probe_stop();
    }
    let inputs = unsafe { runtime_inputs(state, pins, state.motion_commands.ready()) };
    if let Some(error) = advance_error {
        state.runtime.fail(error.fault_code());
        state.motion_commands.force_stop_immediate();
    }
    let mut outputs = state.runtime.update(period_ns, inputs);
    if let Some(command) = outputs.supervisor.command {
        if let Err(error) =
            state
                .motion_commands
                .accept(command, dmc2_core::PULSES_PER_MM, period_ns)
        {
            state.runtime.fail(error.fault_code());
            state.motion_commands.force_stop_immediate();
            outputs.supervisor = state.runtime.supervisor().outputs();
            outputs.limit_reset = outputs.supervisor.limit_reset;
        }
    }
    unsafe { publish(pins, outputs, &state.motion_commands) };
}
