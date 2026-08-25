use core::ffi::{c_long, c_void};

use dmc2_core::supervisor::FaultCode;

use super::hal::{publish, runtime_inputs};
use super::state::ComponentState;

pub(super) unsafe extern "C" fn update_component(argument: *mut c_void, period: c_long) {
    if argument.is_null() || period <= 0 {
        return;
    }
    let state = unsafe { &mut *argument.cast::<ComponentState>() };
    let pins = unsafe { &*state.pins };
    let period_ns = period as u64;
    state.sequencer.advance(period_ns);
    let inputs = unsafe { runtime_inputs(state, pins, state.sequencer.ready()) };
    let mut outputs = state.runtime.update(period_ns, inputs);
    if let Some(command) = outputs.supervisor.command {
        if !state.sequencer.accept(command) {
            state.runtime.fail(FaultCode::CommandSequencerFailure);
            state.sequencer.force_stop_immediate();
            outputs.supervisor = state.runtime.supervisor().outputs();
            outputs.limit_reset = outputs.supervisor.limit_reset;
        }
    }
    unsafe { publish(pins, outputs, &state.sequencer) };
}
