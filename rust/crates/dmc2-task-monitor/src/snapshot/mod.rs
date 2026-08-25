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
    dmc2_rcs_status_snapshot as RcsStatusSnapshot, dmc2_task_status_snapshot as NativeSnapshot,
};

pub(crate) use abi::{
    dmc2_task_status_channel as NativeTaskStatusChannel, dmc2_task_status_close,
    dmc2_task_status_open, dmc2_task_status_poll, dmc2_task_status_snapshot_abi_version,
    dmc2_task_status_snapshot_size,
};

#[cfg(test)]
pub(crate) use abi::dmc2_task_status_snapshot_initialize;

pub const SNAPSHOT_ABI_VERSION: u32 = abi::DMC2_SNAPSHOT_ABI_VERSION;

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
}
