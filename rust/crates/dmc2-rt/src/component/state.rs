use dmc2_core::motion::NativeMotionCommandChannel;
use dmc2_core::runtime::RuntimeController;
use dmc2_core::supervisor::MachineSnapshot;

use super::hal::Pins;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CachedTaskSnapshot {
    pub(super) connected: bool,
    pub(super) fault: bool,
    pub(super) heartbeat: u32,
    pub(super) machine: MachineSnapshot,
}

impl CachedTaskSnapshot {
    pub(in crate::component) const fn safe() -> Self {
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
    pub(super) motion_commands: NativeMotionCommandChannel,
    pub(super) task: CachedTaskSnapshot,
}

impl ComponentState {
    pub(super) const fn new(pins: *mut Pins) -> Self {
        Self {
            pins,
            runtime: RuntimeController::new(),
            motion_commands: NativeMotionCommandChannel::new(),
            task: CachedTaskSnapshot::safe(),
        }
    }
}
