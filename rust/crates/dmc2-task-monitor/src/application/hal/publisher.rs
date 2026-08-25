use std::ffi::c_int;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

use dmc2_hal_sys as hal;
use dmc2_linuxcnc_interface::{CMS_STATUS, NML_ERROR, TASK_INTERP, TASK_MODE, TRAJ_MODE};

use crate::application::diagnostic_state::DiagnosticState;
use crate::application::nml::TransportStatus;
use crate::diagnostics::DiagnosticReport;
use crate::snapshot::NativeSnapshot;

use super::pins::HalPins;
use super::registration::create_hal;

pub(in crate::application) struct HalPublisher {
    component_id: c_int,
    pins: *mut HalPins,
    publications: AtomicU32,
}

impl HalPublisher {
    pub(in crate::application) fn new(component: &str) -> Result<Self, String> {
        let (component_id, pins) = unsafe { create_hal(component)? };
        Ok(Self {
            component_id,
            pins,
            publications: AtomicU32::new(0),
        })
    }

    pub(in crate::application) fn increment_poll_errors(&self) {
        let pins = unsafe { &*self.pins };
        unsafe {
            let errors = ptr::read_volatile(pins.poll_errors).wrapping_add(1);
            ptr::write_volatile(pins.poll_errors, errors);
        }
    }

    pub(in crate::application) fn publish(
        &self,
        snapshot: NativeSnapshot,
        connected: bool,
        fault: bool,
        transport: TransportStatus,
        diagnostics: &DiagnosticReport,
        diagnostic_state: &mut DiagnosticState,
    ) {
        let pins = unsafe { &*self.pins };
        let clear_latched = unsafe { ptr::read_volatile(pins.clear_latched) };
        diagnostic_state.update(diagnostics, clear_latched);
        let publications = self
            .publications
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        let generation = publications.wrapping_shl(1);
        let generation_pin = unsafe { &*(pins.snapshot_generation.cast::<AtomicU32>()) };
        unsafe {
            generation_pin.store(generation | 1, Ordering::SeqCst);
            ptr::write_volatile(pins.task_heartbeat, snapshot.task.heartbeat);
            ptr::write_volatile(pins.machine_on, snapshot.trajectory.enabled != 0);
            ptr::write_volatile(pins.estopped, snapshot.io.aux.estop != 0);
            ptr::write_volatile(
                pins.manual_mode,
                TASK_MODE.lookup(i64::from(snapshot.task.mode)) == Some("EMC_TASK_MODE_MANUAL"),
            );
            ptr::write_volatile(
                pins.joint_mode,
                TRAJ_MODE.lookup(i64::from(snapshot.trajectory.mode)) == Some("EMC_TRAJ_MODE_FREE"),
            );
            ptr::write_volatile(
                pins.teleop_mode,
                TRAJ_MODE.lookup(i64::from(snapshot.trajectory.mode))
                    == Some("EMC_TRAJ_MODE_TELEOP"),
            );
            ptr::write_volatile(
                pins.interp_idle,
                TASK_INTERP.lookup(i64::from(snapshot.task.interp_state))
                    == Some("EMC_TASK_INTERP_IDLE"),
            );
            for index in 0..3 {
                ptr::write_volatile(pins.homed[index], snapshot.joints[index].homed != 0);
                ptr::write_volatile(pins.homing[index], snapshot.joints[index].homing != 0);
                ptr::write_volatile(pins.axis_stopped[index], snapshot.axes[index].stopped != 0);
            }
            ptr::write_volatile(pins.connected, connected);
            ptr::write_volatile(pins.fault, fault);
            ptr::write_volatile(pins.nml_error_code, transport.nml_error);
            ptr::write_volatile(
                pins.nml_error_known,
                NML_ERROR.lookup(i64::from(transport.nml_error)).is_some(),
            );
            ptr::write_volatile(pins.cms_status_code, transport.cms_status);
            ptr::write_volatile(
                pins.cms_status_known,
                CMS_STATUS.lookup(i64::from(transport.cms_status)).is_some(),
            );
            ptr::write_volatile(pins.linuxcnc_error_active, diagnostics.error_active());
            ptr::write_volatile(pins.linuxcnc_warning_active, diagnostics.warning_active());
            ptr::write_volatile(pins.unknown_code_active, diagnostics.unknown_code_active());
            ptr::write_volatile(
                pins.active_error_mask_low,
                diagnostics.active_error_mask as u32,
            );
            ptr::write_volatile(
                pins.active_error_mask_high,
                (diagnostics.active_error_mask >> 32) as u32,
            );
            ptr::write_volatile(
                pins.active_warning_mask_low,
                diagnostics.active_warning_mask as u32,
            );
            ptr::write_volatile(
                pins.active_warning_mask_high,
                (diagnostics.active_warning_mask >> 32) as u32,
            );
            ptr::write_volatile(
                pins.latched_error_mask_low,
                diagnostic_state.latched_error_mask as u32,
            );
            ptr::write_volatile(
                pins.latched_error_mask_high,
                (diagnostic_state.latched_error_mask >> 32) as u32,
            );
            ptr::write_volatile(
                pins.latched_warning_mask_low,
                diagnostic_state.latched_warning_mask as u32,
            );
            ptr::write_volatile(
                pins.latched_warning_mask_high,
                (diagnostic_state.latched_warning_mask >> 32) as u32,
            );
            ptr::write_volatile(
                pins.unknown_domain_mask_low,
                diagnostics.unknown_domain_mask as u32,
            );
            ptr::write_volatile(
                pins.unknown_domain_mask_high,
                (diagnostics.unknown_domain_mask >> 32) as u32,
            );
            ptr::write_volatile(
                pins.diagnostic_count,
                diagnostics.issues.len().try_into().unwrap_or(u32::MAX),
            );
            ptr::write_volatile(pins.unknown_code_count, diagnostics.unknown_code_count());
            ptr::write_volatile(pins.diagnostic_transitions, diagnostic_state.transitions);
            ptr::write_volatile(pins.latest_code_domain, diagnostic_state.latest_code_domain);
            ptr::write_volatile(pins.latest_code_low, diagnostic_state.latest_code_low);
            ptr::write_volatile(pins.latest_code_high, diagnostic_state.latest_code_high);
            ptr::write_volatile(pins.latest_severity, diagnostic_state.latest_severity);
            ptr::write_volatile(pins.latest_action, diagnostic_state.latest_action);
            ptr::write_volatile(pins.publications, publications);
            generation_pin.store(generation, Ordering::SeqCst);
        }
    }

    #[cfg(test)]
    pub(super) fn test_pins(&self) -> &HalPins {
        unsafe { &*self.pins }
    }
}

impl Drop for HalPublisher {
    fn drop(&mut self) {
        if let Err(error) = hal::HalCall::Exit.classify(unsafe { hal::hal_exit(self.component_id) })
        {
            eprintln!("dmc2-task-monitor: hal_exit failed: {error}");
        }
    }
}
