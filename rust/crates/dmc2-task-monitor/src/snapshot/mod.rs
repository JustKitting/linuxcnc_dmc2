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
    dmc2_task_status_copy_self_test, dmc2_task_status_copy_signature_rounds, dmc2_task_status_open,
    dmc2_task_status_poll, dmc2_task_status_snapshot_abi_version, dmc2_task_status_snapshot_size,
};

#[cfg(test)]
pub(crate) use abi::dmc2_task_status_snapshot_initialize;

pub const SNAPSHOT_ABI_VERSION: u32 = abi::DMC2_SNAPSHOT_ABI_VERSION;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SnapshotFieldSpec {
    pub path: &'static str,
    pub c_type: &'static str,
    pub element_count: usize,
    pub byte_offset: usize,
    pub byte_size: usize,
}

include!(concat!(env!("OUT_DIR"), "/status_snapshot_fields.rs"));

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
    fn native_copy_maps_every_logical_field_and_every_destination_byte() {
        let mut logical_fields = 0_u32;
        let mut failure_offset = usize::MAX;
        let result =
            unsafe { dmc2_task_status_copy_self_test(&mut logical_fields, &mut failure_offset) };
        assert_eq!(
            result, 0,
            "native status copy failed in signature round {result} at destination byte {failure_offset}"
        );
        assert_eq!(logical_fields as usize, SNAPSHOT_LOGICAL_FIELD_COUNT);
        assert_eq!(unsafe { dmc2_task_status_copy_signature_rounds() }, 21);
    }
}
