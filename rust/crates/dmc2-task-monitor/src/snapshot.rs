use dmc2_linuxcnc_interface::{
    EMCMOT_MAX_AXIS, EMCMOT_MAX_JOINTS, EMCMOT_MAX_MISC_ERROR, EMCMOT_MAX_SPINDLES,
};

pub const SNAPSHOT_ABI_VERSION: u32 = 0x0002_0910;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct RcsStatusSnapshot {
    pub command_type: i64,
    pub echo_serial_number: i32,
    pub status: i32,
    pub state: i32,
    pub reserved: i32,
}

impl RcsStatusSnapshot {
    pub const fn zeroed() -> Self {
        Self {
            command_type: 0,
            echo_serial_number: 0,
            status: 0,
            state: 0,
            reserved: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct TaskSnapshot {
    pub rcs: RcsStatusSnapshot,
    pub heartbeat: u32,
    pub mode: i32,
    pub state: i32,
    pub exec_state: i32,
    pub interp_state: i32,
    pub program_units: i32,
    pub interpreter_errcode: i32,
    pub input_timeout: u32,
    pub paused: u32,
}

impl TaskSnapshot {
    pub const fn zeroed() -> Self {
        Self {
            rcs: RcsStatusSnapshot::zeroed(),
            heartbeat: 0,
            mode: 0,
            state: 0,
            exec_state: 0,
            interp_state: 0,
            program_units: 0,
            interpreter_errcode: 0,
            input_timeout: 0,
            paused: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct TrajectorySnapshot {
    pub rcs: RcsStatusSnapshot,
    pub joints: i32,
    pub spindles: i32,
    pub axis_mask: i32,
    pub mode: i32,
    pub kinematics_type: i32,
    pub motion_type: i32,
    pub enabled: u32,
    pub in_position: u32,
    pub queue_full: u32,
    pub paused: u32,
    pub probe_tripped: u32,
    pub probing: u32,
    pub probe_value: i32,
    pub feed_override_enabled: u32,
    pub adaptive_feed_enabled: u32,
    pub feed_hold_enabled: u32,
    pub state_tag_flags: u64,
}

impl TrajectorySnapshot {
    pub const fn zeroed() -> Self {
        Self {
            rcs: RcsStatusSnapshot::zeroed(),
            joints: 0,
            spindles: 0,
            axis_mask: 0,
            mode: 0,
            kinematics_type: 0,
            motion_type: 0,
            enabled: 0,
            in_position: 0,
            queue_full: 0,
            paused: 0,
            probe_tripped: 0,
            probing: 0,
            probe_value: 0,
            feed_override_enabled: 0,
            adaptive_feed_enabled: 0,
            feed_hold_enabled: 0,
            state_tag_flags: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct JointSnapshot {
    pub rcs: RcsStatusSnapshot,
    pub joint_type: i32,
    pub in_position: u32,
    pub homing: u32,
    pub homed: u32,
    pub fault: u32,
    pub enabled: u32,
    pub min_soft_limit: u32,
    pub max_soft_limit: u32,
    pub min_hard_limit: u32,
    pub max_hard_limit: u32,
    pub override_limits: u32,
}

impl JointSnapshot {
    pub const fn zeroed() -> Self {
        Self {
            rcs: RcsStatusSnapshot::zeroed(),
            joint_type: 0,
            in_position: 0,
            homing: 0,
            homed: 0,
            fault: 0,
            enabled: 0,
            min_soft_limit: 0,
            max_soft_limit: 0,
            min_hard_limit: 0,
            max_hard_limit: 0,
            override_limits: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct AxisSnapshot {
    pub rcs: RcsStatusSnapshot,
    pub stopped: u32,
}

impl AxisSnapshot {
    pub const fn zeroed() -> Self {
        Self {
            rcs: RcsStatusSnapshot::zeroed(),
            stopped: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct SpindleSnapshot {
    pub rcs: RcsStatusSnapshot,
    pub direction: i32,
    pub brake: i32,
    pub enabled: i32,
    pub orient_state: i32,
    pub orient_fault: i32,
    pub override_enabled: u32,
}

impl SpindleSnapshot {
    pub const fn zeroed() -> Self {
        Self {
            rcs: RcsStatusSnapshot::zeroed(),
            direction: 0,
            brake: 0,
            enabled: 0,
            orient_state: 0,
            orient_fault: 0,
            override_enabled: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct IoSnapshot {
    pub rcs: RcsStatusSnapshot,
    pub tool_rcs: RcsStatusSnapshot,
    pub aux_rcs: RcsStatusSnapshot,
    pub coolant_rcs: RcsStatusSnapshot,
    pub lube_rcs: RcsStatusSnapshot,
    pub heartbeat: u32,
    pub debug: i32,
    pub reason: i32,
    pub fault: i32,
    pub estop: i32,
    pub coolant_mist: i32,
    pub coolant_flood: i32,
    pub lube_on: i32,
    pub lube_level: i32,
}

impl IoSnapshot {
    pub const fn zeroed() -> Self {
        Self {
            rcs: RcsStatusSnapshot::zeroed(),
            tool_rcs: RcsStatusSnapshot::zeroed(),
            aux_rcs: RcsStatusSnapshot::zeroed(),
            coolant_rcs: RcsStatusSnapshot::zeroed(),
            lube_rcs: RcsStatusSnapshot::zeroed(),
            heartbeat: 0,
            debug: 0,
            reason: 0,
            fault: 0,
            estop: 1,
            coolant_mist: 0,
            coolant_flood: 0,
            lube_on: 0,
            lube_level: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct NativeSnapshot {
    pub abi_version: u32,
    pub struct_size: u32,
    pub top_rcs: RcsStatusSnapshot,
    pub task: TaskSnapshot,
    pub motion_rcs: RcsStatusSnapshot,
    pub motion_heartbeat: u32,
    pub motion_debug: i32,
    pub num_extra_joints: i32,
    pub jogging_active: u32,
    pub trajectory: TrajectorySnapshot,
    pub joints: [JointSnapshot; EMCMOT_MAX_JOINTS],
    pub axes: [AxisSnapshot; EMCMOT_MAX_AXIS],
    pub spindles: [SpindleSnapshot; EMCMOT_MAX_SPINDLES],
    pub misc_error: [i32; EMCMOT_MAX_MISC_ERROR],
    pub io: IoSnapshot,
    pub top_debug: i32,
}

impl NativeSnapshot {
    pub const fn safe() -> Self {
        Self {
            abi_version: SNAPSHOT_ABI_VERSION,
            struct_size: core::mem::size_of::<Self>() as u32,
            top_rcs: RcsStatusSnapshot::zeroed(),
            task: TaskSnapshot::zeroed(),
            motion_rcs: RcsStatusSnapshot::zeroed(),
            motion_heartbeat: 0,
            motion_debug: 0,
            num_extra_joints: 0,
            jogging_active: 0,
            trajectory: TrajectorySnapshot::zeroed(),
            joints: [JointSnapshot::zeroed(); EMCMOT_MAX_JOINTS],
            axes: [AxisSnapshot::zeroed(); EMCMOT_MAX_AXIS],
            spindles: [SpindleSnapshot::zeroed(); EMCMOT_MAX_SPINDLES],
            misc_error: [0; EMCMOT_MAX_MISC_ERROR],
            io: IoSnapshot::zeroed(),
            top_debug: 0,
        }
    }

    pub fn valid_abi(&self) -> bool {
        self.abi_version == SNAPSHOT_ABI_VERSION
            && self.struct_size as usize == core::mem::size_of::<Self>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_snapshot_is_fail_closed_for_machine_state() {
        let snapshot = NativeSnapshot::safe();
        assert!(snapshot.valid_abi());
        assert_ne!(snapshot.io.estop, 0);
        assert_eq!(snapshot.trajectory.enabled, 0);
        assert!(snapshot.axes.iter().all(|axis| axis.stopped != 0));
    }
}
