use dmc2_linuxcnc_interface::EMCMOT_MAX_AXIS;

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
    dmc2_task_status_copy, dmc2_task_status_copy_self_test, dmc2_task_status_copy_signature_rounds,
    dmc2_task_status_observe, dmc2_task_status_open, dmc2_task_status_snapshot_abi_version,
    dmc2_task_status_snapshot_size, DMC2_TASK_STATUS_NATIVE_OK,
};

#[cfg(test)]
pub(crate) use abi::{dmc2_task_status_snapshot_initialize, DMC2_TASK_STATUS_NATIVE_ERROR};

pub const SNAPSHOT_ABI_VERSION: u32 = abi::DMC2_SNAPSHOT_ABI_VERSION;
pub(crate) const RUST_DERIVED_FIELD_COUNT: usize = EMCMOT_MAX_AXIS;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SnapshotFieldSpec {
    pub path: &'static str,
    pub c_type: &'static str,
    pub element_count: usize,
    pub byte_offset: usize,
    pub byte_size: usize,
}

include!(concat!(env!("OUT_DIR"), "/status_snapshot_fields.rs"));

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

fn extend_schema_fnv(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    hash
}

fn hash_schema_usize(hash: u64, value: usize) -> u64 {
    extend_schema_fnv(
        hash,
        &u64::try_from(value)
            .expect("snapshot layout value exceeds u64")
            .to_le_bytes(),
    )
}

fn hash_schema_text(hash: u64, value: &str) -> u64 {
    let hash = hash_schema_usize(hash, value.len());
    extend_schema_fnv(hash, value.as_bytes())
}

pub(crate) fn snapshot_schema_fingerprint() -> u64 {
    let mut hash = extend_schema_fnv(0xcbf29ce484222325, b"DMC2_SNAPSHOT_LAYOUT_V1\0");
    hash = extend_schema_fnv(hash, &SNAPSHOT_ABI_VERSION.to_le_bytes());
    hash = hash_schema_usize(hash, SNAPSHOT_SCHEMA_STRUCT_SIZE);
    hash = hash_schema_usize(hash, SNAPSHOT_SCHEMA_STRUCT_ALIGNMENT);
    hash = hash_schema_usize(hash, SNAPSHOT_FIELDS.len());
    for field in SNAPSHOT_FIELDS {
        hash = hash_schema_text(hash, field.path);
        hash = hash_schema_text(hash, field.c_type);
        hash = hash_schema_usize(hash, field.element_count);
        hash = hash_schema_usize(hash, field.byte_offset);
        hash = hash_schema_usize(hash, field.byte_size);
    }
    hash
}

impl NativeSnapshot {
    pub fn safe() -> Self {
        let mut snapshot = Self::default();
        snapshot.abi_version = SNAPSHOT_ABI_VERSION;
        snapshot.struct_size = core::mem::size_of::<Self>()
            .try_into()
            .expect("native snapshot size exceeds its u32 ABI field");
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

#[cfg(test)]
mod tests {
    use core::mem::{self, MaybeUninit};
    use core::slice;

    use dmc2_linuxcnc_interface::{
        EMCMOT_MAX_AXIS, EMCMOT_MAX_JOINTS, EMCMOT_MAX_MISC_ERROR, EMCMOT_MAX_SPINDLES,
    };

    use super::*;

    fn bytes(snapshot: &NativeSnapshot) -> &[u8] {
        unsafe {
            slice::from_raw_parts(
                core::ptr::from_ref(snapshot).cast::<u8>(),
                mem::size_of::<NativeSnapshot>(),
            )
        }
    }

    #[test]
    fn safe_snapshot_is_fail_closed_for_machine_state() {
        let snapshot = NativeSnapshot::safe();
        assert!(snapshot.valid_abi());
        assert_ne!(snapshot.io.aux.estop, 0);
        assert_eq!(snapshot.trajectory.enabled, 0);
        assert!(snapshot.axes.iter().all(|axis| axis.stopped != 0));
    }

    #[test]
    fn native_initializer_overwrites_every_byte_deterministically() {
        let mut first = MaybeUninit::<NativeSnapshot>::uninit();
        let mut second = MaybeUninit::<NativeSnapshot>::uninit();
        unsafe {
            first
                .as_mut_ptr()
                .cast::<u8>()
                .write_bytes(0x55, mem::size_of::<NativeSnapshot>());
            second
                .as_mut_ptr()
                .cast::<u8>()
                .write_bytes(0xaa, mem::size_of::<NativeSnapshot>());
            dmc2_task_status_snapshot_initialize(first.as_mut_ptr());
            dmc2_task_status_snapshot_initialize(second.as_mut_ptr());
            let first = first.assume_init();
            let second = second.assume_init();
            assert_eq!(bytes(&first), bytes(&second));
            assert!(first.valid_abi());
        }
    }

    #[test]
    fn native_and_rust_abi_versions_and_sizes_are_identical() {
        assert_eq!(
            unsafe { dmc2_task_status_snapshot_abi_version() },
            SNAPSHOT_ABI_VERSION
        );
        assert_eq!(
            unsafe { dmc2_task_status_snapshot_size() },
            mem::size_of::<NativeSnapshot>()
        );
    }

    #[test]
    fn generated_snapshot_extents_match_the_source_catalog() {
        assert_eq!(abi::DMC2_MAX_JOINTS as usize, EMCMOT_MAX_JOINTS);
        assert_eq!(abi::DMC2_MAX_AXES as usize, EMCMOT_MAX_AXIS);
        assert_eq!(abi::DMC2_MAX_SPINDLES as usize, EMCMOT_MAX_SPINDLES);
        assert_eq!(abi::DMC2_MAX_MISC_ERRORS as usize, EMCMOT_MAX_MISC_ERROR);
        assert_eq!(NativeSnapshot::default().joints.len(), EMCMOT_MAX_JOINTS);
        assert_eq!(NativeSnapshot::default().axes.len(), EMCMOT_MAX_AXIS);
        assert_eq!(
            NativeSnapshot::default().spindles.len(),
            EMCMOT_MAX_SPINDLES
        );
        assert_eq!(
            NativeSnapshot::default().misc_error.len(),
            EMCMOT_MAX_MISC_ERROR
        );
    }

    #[test]
    fn generated_field_inventory_accounts_for_every_data_and_padding_byte() {
        assert_eq!(SNAPSHOT_FIELDS.len(), SNAPSHOT_LOGICAL_FIELD_COUNT);
        assert_ne!(SNAPSHOT_SCHEMA_FNV64, 0);
        assert_eq!(
            SNAPSHOT_SCHEMA_STRUCT_SIZE,
            mem::size_of::<NativeSnapshot>()
        );
        assert_eq!(
            SNAPSHOT_SCHEMA_STRUCT_ALIGNMENT,
            mem::align_of::<NativeSnapshot>()
        );
        assert_eq!(SNAPSHOT_SCHEMA_FNV64, snapshot_schema_fingerprint());
        let mut owners = vec![None; mem::size_of::<NativeSnapshot>()];
        for field in SNAPSHOT_FIELDS {
            assert!(!field.path.is_empty());
            assert!(!field.c_type.is_empty());
            assert!(field.element_count > 0);
            assert!(field.byte_size > 0);
            let end = field
                .byte_offset
                .checked_add(field.byte_size)
                .expect("snapshot field range overflowed");
            assert!(
                end <= owners.len(),
                "field outside snapshot: {}",
                field.path
            );
            for owner in &mut owners[field.byte_offset..end] {
                assert!(
                    owner.is_none(),
                    "overlapping snapshot field: {}",
                    field.path
                );
                *owner = Some(field.path);
            }
        }
        let field_bytes = owners.iter().filter(|owner| owner.is_some()).count();
        let padding_bytes = owners.len() - field_bytes;
        assert!(field_bytes > 0);
        assert_eq!(
            field_bytes + padding_bytes,
            mem::size_of::<NativeSnapshot>()
        );
    }

    #[test]
    fn native_copy_and_rust_derivation_own_every_logical_field() {
        let mut native_copy_fields = 0_u32;
        let mut failure_offset = usize::MAX;
        let result = unsafe {
            dmc2_task_status_copy_self_test(&mut native_copy_fields, &mut failure_offset)
        };
        assert_eq!(
            result, 0,
            "native status copy failed in signature round {result} at destination byte {failure_offset}"
        );
        assert_eq!(native_copy_fields, 1_100);
        assert_eq!(RUST_DERIVED_FIELD_COUNT, 9);
        assert_eq!(
            native_copy_fields as usize + RUST_DERIVED_FIELD_COUNT,
            SNAPSHOT_LOGICAL_FIELD_COUNT
        );
        assert_eq!(unsafe { dmc2_task_status_copy_signature_rounds() }, 21);
    }

    #[test]
    fn inactive_axes_are_stopped_independent_of_velocity_and_joint_state() {
        let mut snapshot = NativeSnapshot::safe();
        snapshot.trajectory.axis_mask = 0;
        snapshot.trajectory.joints = snapshot.joints.len() as i32;
        for index in 0..snapshot.axes.len() {
            snapshot.axes[index].stopped = 0;
            snapshot.axes[index].velocity = f64::NAN;
            snapshot.joints[index].in_position = 0;
            snapshot.joints[index].velocity = f64::NAN;
        }

        derive_axis_stopped(&mut snapshot);

        assert!(snapshot.axes.iter().all(|axis| axis.stopped == 1));
    }

    #[test]
    fn active_axis_rule_preserves_every_velocity_and_in_position_boundary() {
        let velocities = [
            -STOPPED_VELOCITY_TOLERANCE,
            STOPPED_VELOCITY_TOLERANCE,
            -STOPPED_VELOCITY_TOLERANCE - f64::EPSILON,
            STOPPED_VELOCITY_TOLERANCE + f64::EPSILON,
            f64::NAN,
        ];
        for in_position in [0_u32, 1_u32] {
            for joint_velocity in velocities {
                for axis_velocity in velocities {
                    let mut snapshot = NativeSnapshot::safe();
                    snapshot.trajectory.axis_mask = 1;
                    snapshot.trajectory.joints = 1;
                    snapshot.joints[0].in_position = in_position;
                    snapshot.joints[0].velocity = joint_velocity;
                    snapshot.axes[0].velocity = axis_velocity;

                    derive_axis_stopped(&mut snapshot);

                    let expected = in_position != 0
                        && velocity_is_stopped(joint_velocity)
                        && velocity_is_stopped(axis_velocity);
                    assert_eq!(
                        snapshot.axes[0].stopped,
                        u32::from(expected),
                        "in_position={in_position} joint_velocity={joint_velocity:?} axis_velocity={axis_velocity:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn active_axis_without_a_reported_joint_uses_axis_velocity_only() {
        for joint_count in [i32::MIN, -1, 0] {
            let mut snapshot = NativeSnapshot::safe();
            snapshot.trajectory.axis_mask = 1;
            snapshot.trajectory.joints = joint_count;
            snapshot.axes[0].velocity = STOPPED_VELOCITY_TOLERANCE;
            derive_axis_stopped(&mut snapshot);
            assert_eq!(snapshot.axes[0].stopped, 1);

            snapshot.axes[0].velocity = STOPPED_VELOCITY_TOLERANCE + f64::EPSILON;
            derive_axis_stopped(&mut snapshot);
            assert_eq!(snapshot.axes[0].stopped, 0);
        }
    }

    #[test]
    fn derivation_overwrites_every_axis_slot() {
        let mut snapshot = NativeSnapshot::safe();
        snapshot.trajectory.axis_mask = (1_i32 << snapshot.axes.len()) - 1;
        snapshot.trajectory.joints = snapshot.axes.len() as i32;
        for index in 0..snapshot.axes.len() {
            snapshot.axes[index].stopped = u32::MAX;
            snapshot.axes[index].velocity = if index % 2 == 0 {
                0.0
            } else {
                STOPPED_VELOCITY_TOLERANCE * 2.0
            };
            snapshot.joints[index].in_position = 1;
            snapshot.joints[index].velocity = 0.0;
        }

        derive_axis_stopped(&mut snapshot);

        for (index, axis) in snapshot.axes.iter().enumerate() {
            assert_eq!(axis.stopped, u32::from(index % 2 == 0));
        }
    }
}
