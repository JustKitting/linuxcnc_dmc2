use super::{FaultCode, Phase};
use crate::pendant::PendantSample;
use crate::Axis;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkSnapshot {
    pub connected: bool,
    pub serial_fault: bool,
    pub quadrature_fault: bool,
    pub estop_pressed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MachineSnapshot {
    pub machine_on: bool,
    pub estopped: bool,
    pub manual_mode: bool,
    pub joint_mode: bool,
    pub teleop_mode: bool,
    pub interp_idle: bool,
    pub homed: [bool; 3],
    pub homing: [bool; 3],
    pub axis_stopped: [bool; 3],
}

impl MachineSnapshot {
    pub const fn all_homed(&self) -> bool {
        self.homed[0] && self.homed[1] && self.homed[2]
    }

    pub const fn any_homing(&self) -> bool {
        self.homing[0] || self.homing[1] || self.homing[2]
    }

    pub const fn ready_for_pendant_jog(&self) -> bool {
        let jog_mode_ready = if self.all_homed() {
            self.teleop_mode
        } else {
            self.joint_mode
        };
        self.machine_on
            && !self.estopped
            && self.manual_mode
            && jog_mode_ready
            && self.interp_idle
            && !self.any_homing()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JogCommand {
    pub axis: Axis,
    pub joint_jog: bool,
    pub signed_delta_pulses: f64,
    pub speed_mm_per_minute: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CommandEvent {
    JogIncrement(JogCommand),
    JogStop,
    JogStopImmediate,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SupervisorOutputs {
    pub external_enable: bool,
    pub control_available: bool,
    pub control_ready: bool,
    pub estop_reset_request: bool,
    pub machine_on_request: bool,
    pub fault: Option<FaultCode>,
    pub recovery_active: bool,
    pub jog_active: bool,
    pub bounce_active: bool,
    pub active_axis: Option<Axis>,
    pub phase: Phase,
    pub limit_reset: [bool; 3],
    pub command_enable_by_motor: [bool; 3],
    pub toward_limit_by_motor: [bool; 3],
    pub command: Option<CommandEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SupervisorInputs {
    pub link: LinkSnapshot,
    pub packet: Option<PendantSample>,
    pub machine: MachineSnapshot,
    pub counts_by_motor: [i32; 3],
    pub position_feedback_by_motor: [f64; 3],
    pub raw_limits: [bool; 3],
    pub safety_limits: [bool; 3],
    pub pendant_mode_enabled: bool,
    pub command_channel_ready: bool,
}

#[cfg(test)]
mod tests;
