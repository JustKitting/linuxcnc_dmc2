#[allow(
    dead_code,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals
)]
mod abi {
    include!(concat!(env!("OUT_DIR"), "/status_snapshot_bindings.rs"));
}

pub use abi::{
    dmc2_pose_snapshot as PoseSnapshot, dmc2_rcs_status_snapshot as RcsStatusSnapshot,
    dmc2_task_status_snapshot as NativeSnapshot,
};

pub(crate) use abi::{
    dmc2_task_status_channel as NativeTaskStatusChannel, dmc2_task_status_close,
    dmc2_task_status_copy, dmc2_task_status_observe, dmc2_task_status_open,
    dmc2_task_status_snapshot_abi_version, dmc2_task_status_snapshot_size,
    DMC2_TASK_STATUS_NATIVE_OK,
};

pub const SNAPSHOT_ABI_VERSION: u32 = abi::DMC2_SNAPSHOT_ABI_VERSION;
const STOPPED_VELOCITY_TOLERANCE: f64 = 0.000_001;

const _: unsafe extern "C" fn() -> u32 = dmc2_task_status_snapshot_abi_version;
const _: unsafe extern "C" fn() -> usize = dmc2_task_status_snapshot_size;
const _: unsafe extern "C" fn(
    *const core::ffi::c_char,
    *mut i32,
    *mut i32,
) -> *mut NativeTaskStatusChannel = dmc2_task_status_open;
const _: unsafe extern "C" fn(*mut NativeTaskStatusChannel, *mut i32, *mut i32, *mut i32) -> i32 =
    dmc2_task_status_observe;
const _: unsafe extern "C" fn(*mut NativeTaskStatusChannel, *mut NativeSnapshot) -> i32 =
    dmc2_task_status_copy;
const _: unsafe extern "C" fn(*mut NativeTaskStatusChannel) = dmc2_task_status_close;

fn velocity_is_stopped(velocity: f64) -> bool {
    (-STOPPED_VELOCITY_TOLERANCE..=STOPPED_VELOCITY_TOLERANCE).contains(&velocity)
}

pub(crate) fn derive_axis_stopped(snapshot: &mut NativeSnapshot) {
    let axis_mask = snapshot.trajectory.axis_mask;
    let joint_count = snapshot.trajectory.joints;

    for index in 0..snapshot.axes.len() {
        let axis_active = axis_mask & (1_i32 << index) != 0;
        if !axis_active {
            snapshot.axes[index].stopped = 1;
            continue;
        }

        let joint_stopped = index as i32 >= joint_count
            || (snapshot.joints[index].in_position != 0
                && velocity_is_stopped(snapshot.joints[index].velocity));
        snapshot.axes[index].stopped =
            u32::from(joint_stopped && velocity_is_stopped(snapshot.axes[index].velocity));
    }
}

impl NativeSnapshot {
    pub fn safe() -> Self {
        let mut snapshot = Self {
            abi_version: SNAPSHOT_ABI_VERSION,
            struct_size: core::mem::size_of::<Self>()
                .try_into()
                .expect("native snapshot size exceeds its u32 ABI field"),
            ..Self::default()
        };
        snapshot.io.aux.estop = 1;
        for axis in &mut snapshot.axes {
            axis.stopped = 1;
        }
        snapshot
    }

    pub fn valid_abi(&self) -> bool {
        self.abi_version == SNAPSHOT_ABI_VERSION
            && self.struct_size as usize == core::mem::size_of::<Self>()
    }
}
