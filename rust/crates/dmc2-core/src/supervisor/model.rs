use super::{FaultCode, FaultRecord, Phase};
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

/// Realtime state published by LinuxCNC motion itself.
///
/// Unlike the task snapshot, these values are produced and consumed on the
/// servo thread.  They are the acknowledgement boundary for native wheel-jog
/// commands and stop requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionSnapshot {
    pub enabled: bool,
    pub teleop_mode: bool,
    pub coord_mode: bool,
    pub in_position: bool,
    pub jog_active: bool,
    pub axis_wheel_jog_active: [bool; 3],
    pub joint_wheel_jog_active: [bool; 3],
    pub joint_in_position: [bool; 3],
}

impl MotionSnapshot {
    pub const fn ready_for_path(&self, joint_jog: bool) -> bool {
        if joint_jog {
            self.enabled && !self.teleop_mode && !self.coord_mode
        } else {
            self.enabled && self.teleop_mode && !self.coord_mode
        }
    }

    pub const fn wheel_active(&self, axis: Axis, joint_jog: bool) -> bool {
        if joint_jog {
            self.joint_wheel_jog_active[axis.index()]
        } else {
            self.axis_wheel_jog_active[axis.index()]
        }
    }

    /// Consumer-side completion for the selected LinuxCNC wheel-jog path.
    ///
    /// Axis teleop motion does not maintain `motion.in-position` as its wheel
    /// planner completion flag.  `axis.L.wheel-jog-active` is cleared by
    /// LinuxCNC only after that axis teleop planner becomes inactive.  Joint
    /// free motion additionally publishes a per-joint in-position bit.
    pub const fn path_settled(&self, axis: Axis, joint_jog: bool) -> bool {
        if joint_jog {
            !self.joint_wheel_jog_active[axis.index()] && self.joint_in_position[axis.index()]
        } else {
            !self.axis_wheel_jog_active[axis.index()]
        }
    }

    pub const fn all_jogs_stopped(&self) -> bool {
        !self.jog_active
            && !self.axis_wheel_jog_active[0]
            && !self.axis_wheel_jog_active[1]
            && !self.axis_wheel_jog_active[2]
            && !self.joint_wheel_jog_active[0]
            && !self.joint_wheel_jog_active[1]
            && !self.joint_wheel_jog_active[2]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JogCommand {
    pub axis: Axis,
    pub joint_jog: bool,
    pub signed_delta_pulses: f64,
    /// Rate at which finite position targets are issued to LinuxCNC motion.
    /// LinuxCNC's configured planner limits remain the physical speed ceiling.
    pub target_rate_mm_per_minute: i32,
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
    pub fault_record: Option<FaultRecord>,
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
    pub motion: MotionSnapshot,
    pub counts_by_motor: [i32; 3],
    pub position_feedback_by_motor: [f64; 3],
    pub raw_limits: [bool; 3],
    pub safety_limits: [bool; 3],
    pub pendant_mode_enabled: bool,
    /// Local native jog-count publisher availability. This is not a
    /// LinuxCNC acknowledgement; `motion` carries the consumer evidence.
    pub motion_command_ready: bool,
    /// Rising edge from LinuxCNC's canonical E-stop reset request.
    pub linuxcnc_estop_reset_rising: bool,
}
