use dmc2_core::halui::HaluiCommandSequencer;
use dmc2_core::runtime::RuntimeController;
use dmc2_core::supervisor::MachineSnapshot;

use super::hal::Pins;

#[derive(Clone, Copy)]
pub(super) struct CachedTaskSnapshot {
    pub(super) connected: bool,
    pub(super) fault: bool,
    pub(super) heartbeat: u32,
    pub(super) machine: MachineSnapshot,
}

impl CachedTaskSnapshot {
    const fn safe() -> Self {
        Self {
            connected: false,
            fault: true,
            heartbeat: 0,
            machine: MachineSnapshot {
                machine_on: false,
                estopped: true,
                manual_mode: false,
                joint_mode: false,
                teleop_mode: false,
                interp_idle: false,
                homed: [false; 3],
                homing: [false; 3],
                axis_stopped: [true; 3],
            },
        }
    }
}

pub(super) struct ComponentState {
    pub(super) pins: *mut Pins,
    pub(super) runtime: RuntimeController,
    pub(super) sequencer: HaluiCommandSequencer,
    pub(super) task: CachedTaskSnapshot,
}

impl ComponentState {
    pub(super) const fn new(pins: *mut Pins) -> Self {
        Self {
            pins,
            runtime: RuntimeController::new(),
            sequencer: HaluiCommandSequencer::new(),
            task: CachedTaskSnapshot::safe(),
        }
    }
}
