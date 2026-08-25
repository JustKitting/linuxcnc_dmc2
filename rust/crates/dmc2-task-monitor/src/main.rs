use std::env;
use std::ffi::{c_char, c_int, CString};
use std::mem;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

use dmc2_hal_sys as hal;
use dmc2_linuxcnc_interface::{
    GENERATED_CODE_COUNT, HEADER_SOURCE_FNV64, LINUXCNC_SOURCE_COMMIT, LINUXCNC_VERSION, NML_ERROR,
    TASK_INTERP, TASK_MODE, TRAJ_MODE,
};

mod diagnostics;
mod snapshot;

use snapshot::{NativeSnapshot, SNAPSHOT_ABI_VERSION};

use diagnostics::{DiagnosticReport, Severity, TransitionLogger};

const DEFAULT_COMPONENT: &str = "dmc2-task-monitor";
const DEFAULT_NML_FILE: &str = "/usr/share/linuxcnc/linuxcnc.nml";
const POLL_PERIOD: Duration = Duration::from_millis(10);
const RECONNECT_PERIOD: Duration = Duration::from_secs(1);

#[repr(C)]
struct TaskStatusChannel {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn dmc2_task_status_snapshot_abi_version() -> u32;
    fn dmc2_task_status_snapshot_size() -> usize;
    fn dmc2_task_status_open(
        nml_file: *const c_char,
        nml_error: *mut i32,
    ) -> *mut TaskStatusChannel;
    fn dmc2_task_status_poll(
        channel: *mut TaskStatusChannel,
        snapshot: *mut NativeSnapshot,
        nml_error: *mut i32,
    ) -> c_int;
    fn dmc2_task_status_close(channel: *mut TaskStatusChannel);
}

#[derive(Debug)]
struct Arguments {
    component: String,
    nml_file: String,
    validate: bool,
}

fn arguments() -> Result<Arguments, String> {
    let mut component = DEFAULT_COMPONENT.to_owned();
    let mut nml_file = env::var("EMC2_NMLFILE").unwrap_or_else(|_| DEFAULT_NML_FILE.to_owned());
    let mut validate = false;
    let mut items = env::args().skip(1);
    while let Some(argument) = items.next() {
        match argument.as_str() {
            "--component" => {
                component = items
                    .next()
                    .ok_or_else(|| "--component requires a value".to_owned())?;
            }
            "--nml-file" => {
                nml_file = items
                    .next()
                    .ok_or_else(|| "--nml-file requires a value".to_owned())?;
            }
            "--validate" => validate = true,
            "--help" | "-h" => {
                println!(
                    "Usage: dmc2-task-monitor [--component NAME] [--nml-file PATH] [--validate]"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(Arguments {
        component,
        nml_file,
        validate,
    })
}

struct HalPins {
    snapshot_generation: *mut hal::hal_u32_t,
    connected: *mut hal::hal_bit_t,
    fault: *mut hal::hal_bit_t,
    task_heartbeat: *mut hal::hal_u32_t,
    publications: *mut hal::hal_u32_t,
    poll_errors: *mut hal::hal_u32_t,
    nml_error_code: *mut hal::hal_s32_t,
    nml_error_known: *mut hal::hal_bit_t,
    linuxcnc_error_active: *mut hal::hal_bit_t,
    linuxcnc_warning_active: *mut hal::hal_bit_t,
    unknown_code_active: *mut hal::hal_bit_t,
    active_error_mask_low: *mut hal::hal_u32_t,
    active_error_mask_high: *mut hal::hal_u32_t,
    active_warning_mask_low: *mut hal::hal_u32_t,
    active_warning_mask_high: *mut hal::hal_u32_t,
    latched_error_mask_low: *mut hal::hal_u32_t,
    latched_error_mask_high: *mut hal::hal_u32_t,
    latched_warning_mask_low: *mut hal::hal_u32_t,
    latched_warning_mask_high: *mut hal::hal_u32_t,
    unknown_domain_mask_low: *mut hal::hal_u32_t,
    unknown_domain_mask_high: *mut hal::hal_u32_t,
    diagnostic_count: *mut hal::hal_u32_t,
    unknown_code_count: *mut hal::hal_u32_t,
    diagnostic_transitions: *mut hal::hal_u32_t,
    latest_code_domain: *mut hal::hal_s32_t,
    latest_code_low: *mut hal::hal_u32_t,
    latest_code_high: *mut hal::hal_u32_t,
    latest_severity: *mut hal::hal_s32_t,
    latest_action: *mut hal::hal_s32_t,
    clear_latched: *mut hal::hal_bit_t,
    catalog_code_count: *mut hal::hal_u32_t,
    catalog_fingerprint_low: *mut hal::hal_u32_t,
    catalog_fingerprint_high: *mut hal::hal_u32_t,
    snapshot_abi_version: *mut hal::hal_u32_t,
    snapshot_struct_size: *mut hal::hal_u32_t,
    machine_on: *mut hal::hal_bit_t,
    estopped: *mut hal::hal_bit_t,
    manual_mode: *mut hal::hal_bit_t,
    joint_mode: *mut hal::hal_bit_t,
    teleop_mode: *mut hal::hal_bit_t,
    interp_idle: *mut hal::hal_bit_t,
    homed: [*mut hal::hal_bit_t; 3],
    homing: [*mut hal::hal_bit_t; 3],
    axis_stopped: [*mut hal::hal_bit_t; 3],
}

impl HalPins {
    const fn empty() -> Self {
        Self {
            snapshot_generation: ptr::null_mut(),
            connected: ptr::null_mut(),
            fault: ptr::null_mut(),
            task_heartbeat: ptr::null_mut(),
            publications: ptr::null_mut(),
            poll_errors: ptr::null_mut(),
            nml_error_code: ptr::null_mut(),
            nml_error_known: ptr::null_mut(),
            linuxcnc_error_active: ptr::null_mut(),
            linuxcnc_warning_active: ptr::null_mut(),
            unknown_code_active: ptr::null_mut(),
            active_error_mask_low: ptr::null_mut(),
            active_error_mask_high: ptr::null_mut(),
            active_warning_mask_low: ptr::null_mut(),
            active_warning_mask_high: ptr::null_mut(),
            latched_error_mask_low: ptr::null_mut(),
            latched_error_mask_high: ptr::null_mut(),
            latched_warning_mask_low: ptr::null_mut(),
            latched_warning_mask_high: ptr::null_mut(),
            unknown_domain_mask_low: ptr::null_mut(),
            unknown_domain_mask_high: ptr::null_mut(),
            diagnostic_count: ptr::null_mut(),
            unknown_code_count: ptr::null_mut(),
            diagnostic_transitions: ptr::null_mut(),
            latest_code_domain: ptr::null_mut(),
            latest_code_low: ptr::null_mut(),
            latest_code_high: ptr::null_mut(),
            latest_severity: ptr::null_mut(),
            latest_action: ptr::null_mut(),
            clear_latched: ptr::null_mut(),
            catalog_code_count: ptr::null_mut(),
            catalog_fingerprint_low: ptr::null_mut(),
            catalog_fingerprint_high: ptr::null_mut(),
            snapshot_abi_version: ptr::null_mut(),
            snapshot_struct_size: ptr::null_mut(),
            machine_on: ptr::null_mut(),
            estopped: ptr::null_mut(),
            manual_mode: ptr::null_mut(),
            joint_mode: ptr::null_mut(),
            teleop_mode: ptr::null_mut(),
            interp_idle: ptr::null_mut(),
            homed: [ptr::null_mut(); 3],
            homing: [ptr::null_mut(); 3],
            axis_stopped: [ptr::null_mut(); 3],
        }
    }
}

fn required_nml_error(name: &str) -> i32 {
    NML_ERROR
        .codes
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("LinuxCNC 2.9.10 catalog omitted {name}"))
        .code
        .try_into()
        .unwrap_or_else(|_| panic!("LinuxCNC 2.9.10 NML error {name} does not fit in i32"))
}

#[derive(Default)]
struct DiagnosticState {
    logger: TransitionLogger,
    latched_error_mask: u64,
    latched_warning_mask: u64,
    transitions: u32,
    latest_code_domain: i32,
    latest_code_low: u32,
    latest_code_high: u32,
    latest_severity: i32,
    latest_action: i32,
    clear_latched_previous: bool,
}

impl DiagnosticState {
    fn new() -> Self {
        Self {
            latest_code_domain: -1,
            ..Self::default()
        }
    }

    fn update(&mut self, report: &DiagnosticReport, clear_latched: bool) {
        if clear_latched && !self.clear_latched_previous {
            self.latched_error_mask = 0;
            self.latched_warning_mask = 0;
        }
        self.clear_latched_previous = clear_latched;
        self.latched_error_mask |= report.active_error_mask;
        self.latched_warning_mask |= report.active_warning_mask;

        let transition = self.logger.update(report);
        self.transitions = self.transitions.wrapping_add(transition.count);
        if let Some(issue) = transition.latest {
            let value = issue.value as u64;
            self.latest_code_domain = if issue.domain_id == u32::MAX {
                -1
            } else {
                issue.domain_id as i32
            };
            self.latest_code_low = value as u32;
            self.latest_code_high = (value >> 32) as u32;
            self.latest_severity = match issue.severity {
                Severity::Warning => 1,
                Severity::Error => 2,
            };
            self.latest_action = transition.latest_action;
        }
    }
}

unsafe fn new_bit_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::hal_bit_t,
    component_id: c_int,
) -> Result<(), String> {
    unsafe {
        new_bit_pin_with_direction(
            component,
            suffix,
            pointer,
            component_id,
            hal::hal_pin_dir_t_HAL_OUT,
        )
    }
}

unsafe fn new_bit_pin_with_direction(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::hal_bit_t,
    component_id: c_int,
    direction: hal::hal_pin_dir_t,
) -> Result<(), String> {
    let name = CString::new(format!("{component}.{suffix}"))
        .map_err(|_| "HAL pin name contained a NUL byte".to_owned())?;
    let result = unsafe { hal::hal_pin_bit_new(name.as_ptr(), direction, pointer, component_id) };
    if result == 0 {
        Ok(())
    } else {
        Err(format!("hal_pin_bit_new({suffix}) failed: {result}"))
    }
}

unsafe fn new_s32_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::hal_s32_t,
    component_id: c_int,
) -> Result<(), String> {
    let name = CString::new(format!("{component}.{suffix}"))
        .map_err(|_| "HAL pin name contained a NUL byte".to_owned())?;
    let result = unsafe {
        hal::hal_pin_s32_new(
            name.as_ptr(),
            hal::hal_pin_dir_t_HAL_OUT,
            pointer,
            component_id,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(format!("hal_pin_s32_new({suffix}) failed: {result}"))
    }
}

unsafe fn new_u32_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::hal_u32_t,
    component_id: c_int,
) -> Result<(), String> {
    let name = CString::new(format!("{component}.{suffix}"))
        .map_err(|_| "HAL pin name contained a NUL byte".to_owned())?;
    let result = unsafe {
        hal::hal_pin_u32_new(
            name.as_ptr(),
            hal::hal_pin_dir_t_HAL_OUT,
            pointer,
            component_id,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(format!("hal_pin_u32_new({suffix}) failed: {result}"))
    }
}

unsafe fn create_hal(component: &str) -> Result<(c_int, *mut HalPins), String> {
    let component_name = CString::new(component)
        .map_err(|_| "HAL component name contained a NUL byte".to_owned())?;
    let component_id = unsafe { hal::hal_init(component_name.as_ptr()) };
    if component_id < 0 {
        return Err(format!("hal_init failed: {component_id}"));
    }

    let result = (|| {
        let pins_pointer =
            unsafe { hal::hal_malloc(mem::size_of::<HalPins>() as _) } as *mut HalPins;
        if pins_pointer.is_null() {
            return Err("hal_malloc for pin-pointer storage failed".to_owned());
        }
        unsafe { ptr::write(pins_pointer, HalPins::empty()) };
        let pins = unsafe { &mut *pins_pointer };
        unsafe {
            new_u32_pin(
                component,
                "snapshot-generation",
                &mut pins.snapshot_generation,
                component_id,
            )?;
            new_bit_pin(component, "connected", &mut pins.connected, component_id)?;
            new_bit_pin(component, "fault", &mut pins.fault, component_id)?;
            new_u32_pin(
                component,
                "task-heartbeat",
                &mut pins.task_heartbeat,
                component_id,
            )?;
            new_u32_pin(
                component,
                "publications",
                &mut pins.publications,
                component_id,
            )?;
            new_u32_pin(
                component,
                "poll-errors",
                &mut pins.poll_errors,
                component_id,
            )?;
            new_bit_pin(
                component,
                "nml-error-known",
                &mut pins.nml_error_known,
                component_id,
            )?;
            new_bit_pin(
                component,
                "linuxcnc-error-active",
                &mut pins.linuxcnc_error_active,
                component_id,
            )?;
            new_bit_pin(
                component,
                "linuxcnc-warning-active",
                &mut pins.linuxcnc_warning_active,
                component_id,
            )?;
            new_bit_pin(
                component,
                "unknown-code-active",
                &mut pins.unknown_code_active,
                component_id,
            )?;
            for (suffix, pointer) in [
                ("active-error-mask-low", &mut pins.active_error_mask_low),
                ("active-error-mask-high", &mut pins.active_error_mask_high),
                ("active-warning-mask-low", &mut pins.active_warning_mask_low),
                (
                    "active-warning-mask-high",
                    &mut pins.active_warning_mask_high,
                ),
                ("latched-error-mask-low", &mut pins.latched_error_mask_low),
                ("latched-error-mask-high", &mut pins.latched_error_mask_high),
                (
                    "latched-warning-mask-low",
                    &mut pins.latched_warning_mask_low,
                ),
                (
                    "latched-warning-mask-high",
                    &mut pins.latched_warning_mask_high,
                ),
                ("unknown-domain-mask-low", &mut pins.unknown_domain_mask_low),
                (
                    "unknown-domain-mask-high",
                    &mut pins.unknown_domain_mask_high,
                ),
                ("diagnostic-count", &mut pins.diagnostic_count),
                ("unknown-code-count", &mut pins.unknown_code_count),
                ("diagnostic-transitions", &mut pins.diagnostic_transitions),
                ("latest-code-low", &mut pins.latest_code_low),
                ("latest-code-high", &mut pins.latest_code_high),
                ("catalog-code-count", &mut pins.catalog_code_count),
                ("catalog-fingerprint-low", &mut pins.catalog_fingerprint_low),
                (
                    "catalog-fingerprint-high",
                    &mut pins.catalog_fingerprint_high,
                ),
                ("snapshot-abi-version", &mut pins.snapshot_abi_version),
                ("snapshot-struct-size", &mut pins.snapshot_struct_size),
            ] {
                new_u32_pin(component, suffix, pointer, component_id)?;
            }
            for (suffix, pointer) in [
                ("nml-error-code", &mut pins.nml_error_code),
                ("latest-code-domain", &mut pins.latest_code_domain),
                ("latest-severity", &mut pins.latest_severity),
                ("latest-action", &mut pins.latest_action),
            ] {
                new_s32_pin(component, suffix, pointer, component_id)?;
            }
            new_bit_pin_with_direction(
                component,
                "clear-latched",
                &mut pins.clear_latched,
                component_id,
                hal::hal_pin_dir_t_HAL_IN,
            )?;
            new_bit_pin(component, "machine-on", &mut pins.machine_on, component_id)?;
            new_bit_pin(component, "estopped", &mut pins.estopped, component_id)?;
            new_bit_pin(
                component,
                "manual-mode",
                &mut pins.manual_mode,
                component_id,
            )?;
            new_bit_pin(component, "joint-mode", &mut pins.joint_mode, component_id)?;
            new_bit_pin(
                component,
                "teleop-mode",
                &mut pins.teleop_mode,
                component_id,
            )?;
            new_bit_pin(
                component,
                "interp-idle",
                &mut pins.interp_idle,
                component_id,
            )?;
            for index in 0..3 {
                new_bit_pin(
                    component,
                    &format!("joint-{index}-homed"),
                    &mut pins.homed[index],
                    component_id,
                )?;
                new_bit_pin(
                    component,
                    &format!("joint-{index}-homing"),
                    &mut pins.homing[index],
                    component_id,
                )?;
                new_bit_pin(
                    component,
                    &format!("axis-{index}-stopped"),
                    &mut pins.axis_stopped[index],
                    component_id,
                )?;
            }

            ptr::write_volatile(pins.snapshot_generation, 1);
            ptr::write_volatile(pins.connected, false);
            ptr::write_volatile(pins.fault, true);
            ptr::write_volatile(pins.task_heartbeat, 0);
            ptr::write_volatile(pins.publications, 0);
            ptr::write_volatile(pins.poll_errors, 0);
            let initial_nml_error = required_nml_error("NML_INVALID_CONFIGURATION");
            ptr::write_volatile(pins.nml_error_code, initial_nml_error);
            ptr::write_volatile(pins.nml_error_known, true);
            ptr::write_volatile(pins.linuxcnc_error_active, true);
            ptr::write_volatile(pins.linuxcnc_warning_active, false);
            ptr::write_volatile(pins.unknown_code_active, false);
            ptr::write_volatile(pins.active_error_mask_low, 0);
            ptr::write_volatile(pins.active_error_mask_high, 0);
            ptr::write_volatile(pins.active_warning_mask_low, 0);
            ptr::write_volatile(pins.active_warning_mask_high, 0);
            ptr::write_volatile(pins.latched_error_mask_low, 0);
            ptr::write_volatile(pins.latched_error_mask_high, 0);
            ptr::write_volatile(pins.latched_warning_mask_low, 0);
            ptr::write_volatile(pins.latched_warning_mask_high, 0);
            ptr::write_volatile(pins.unknown_domain_mask_low, 0);
            ptr::write_volatile(pins.unknown_domain_mask_high, 0);
            ptr::write_volatile(pins.diagnostic_count, 0);
            ptr::write_volatile(pins.unknown_code_count, 0);
            ptr::write_volatile(pins.diagnostic_transitions, 0);
            ptr::write_volatile(pins.latest_code_domain, -1);
            ptr::write_volatile(pins.latest_code_low, 0);
            ptr::write_volatile(pins.latest_code_high, 0);
            ptr::write_volatile(pins.latest_severity, 0);
            ptr::write_volatile(pins.latest_action, 0);
            ptr::write_volatile(pins.clear_latched, false);
            ptr::write_volatile(pins.catalog_code_count, GENERATED_CODE_COUNT as u32);
            ptr::write_volatile(pins.catalog_fingerprint_low, HEADER_SOURCE_FNV64 as u32);
            ptr::write_volatile(
                pins.catalog_fingerprint_high,
                (HEADER_SOURCE_FNV64 >> 32) as u32,
            );
            ptr::write_volatile(pins.snapshot_abi_version, SNAPSHOT_ABI_VERSION);
            ptr::write_volatile(
                pins.snapshot_struct_size,
                mem::size_of::<NativeSnapshot>() as u32,
            );
            ptr::write_volatile(pins.machine_on, false);
            ptr::write_volatile(pins.estopped, true);
            ptr::write_volatile(pins.manual_mode, false);
            ptr::write_volatile(pins.joint_mode, false);
            ptr::write_volatile(pins.teleop_mode, false);
            ptr::write_volatile(pins.interp_idle, false);
            for index in 0..3 {
                ptr::write_volatile(pins.homed[index], false);
                ptr::write_volatile(pins.homing[index], false);
                ptr::write_volatile(pins.axis_stopped[index], true);
            }
            ptr::write_volatile(pins.snapshot_generation, 0);
        }
        let ready = unsafe { hal::hal_ready(component_id) };
        if ready != 0 {
            return Err(format!("hal_ready failed: {ready}"));
        }
        Ok(pins_pointer)
    })();

    match result {
        Ok(pins) => Ok((component_id, pins)),
        Err(error) => {
            unsafe { hal::hal_exit(component_id) };
            Err(error)
        }
    }
}

unsafe fn publish_snapshot(
    pins: &HalPins,
    snapshot: NativeSnapshot,
    connected: bool,
    fault: bool,
    nml_error: i32,
    diagnostics: &DiagnosticReport,
    diagnostic_state: &mut DiagnosticState,
) {
    let clear_latched = unsafe { ptr::read_volatile(pins.clear_latched) };
    diagnostic_state.update(diagnostics, clear_latched);
    let publications = unsafe { ptr::read_volatile(pins.publications) }.wrapping_add(1);
    let generation = publications.wrapping_shl(1);
    let generation_pin = unsafe { &*(pins.snapshot_generation.cast::<AtomicU32>()) };
    unsafe {
        generation_pin.store(generation | 1, Ordering::SeqCst);
        ptr::write_volatile(pins.task_heartbeat, snapshot.task.heartbeat);
        ptr::write_volatile(pins.machine_on, snapshot.trajectory.enabled != 0);
        ptr::write_volatile(pins.estopped, snapshot.io.estop != 0);
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
            TRAJ_MODE.lookup(i64::from(snapshot.trajectory.mode)) == Some("EMC_TRAJ_MODE_TELEOP"),
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
        ptr::write_volatile(pins.nml_error_code, nml_error);
        ptr::write_volatile(
            pins.nml_error_known,
            NML_ERROR.lookup(i64::from(nml_error)).is_some(),
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

fn run() -> Result<(), String> {
    let args = arguments()?;
    let native_abi = unsafe { dmc2_task_status_snapshot_abi_version() };
    let native_size = unsafe { dmc2_task_status_snapshot_size() };
    if native_abi != SNAPSHOT_ABI_VERSION || native_size != mem::size_of::<NativeSnapshot>() {
        return Err(format!(
            "native snapshot ABI mismatch: C++ version=0x{native_abi:08x} size={native_size}, Rust version=0x{SNAPSHOT_ABI_VERSION:08x} size={}",
            mem::size_of::<NativeSnapshot>()
        ));
    }
    if args.validate {
        println!(
            "dmc2-task-monitor: offline validation passed; LinuxCNC={} source={} catalog_codes={} snapshot_abi=0x{:08x} snapshot_size={}",
            LINUXCNC_VERSION,
            LINUXCNC_SOURCE_COMMIT,
            GENERATED_CODE_COUNT,
            SNAPSHOT_ABI_VERSION,
            mem::size_of::<NativeSnapshot>(),
        );
        return Ok(());
    }
    let nml_file = CString::new(args.nml_file.as_str())
        .map_err(|_| "NML file path contained a NUL byte".to_owned())?;
    let (_component_id, pins_pointer) = unsafe { create_hal(&args.component)? };
    let pins = unsafe { &*pins_pointer };

    let mut channel: *mut TaskStatusChannel = ptr::null_mut();
    let mut diagnostic_state = DiagnosticState::new();
    let no_nml_error = required_nml_error("NML_NO_ERROR");
    let invalid_nml_configuration = required_nml_error("NML_INVALID_CONFIGURATION");

    loop {
        if channel.is_null() {
            let mut nml_error = invalid_nml_configuration;
            channel = unsafe { dmc2_task_status_open(nml_file.as_ptr(), &mut nml_error) };
            if channel.is_null() || nml_error != no_nml_error {
                unsafe {
                    if !channel.is_null() {
                        dmc2_task_status_close(channel);
                        channel = ptr::null_mut();
                    }
                    let errors = ptr::read_volatile(pins.poll_errors).wrapping_add(1);
                    ptr::write_volatile(pins.poll_errors, errors);
                    publish_snapshot(
                        pins,
                        NativeSnapshot::safe(),
                        false,
                        true,
                        nml_error,
                        &diagnostics::disconnected(nml_error),
                        &mut diagnostic_state,
                    );
                }
                thread::sleep(RECONNECT_PERIOD);
                continue;
            }
        }
        let mut snapshot = NativeSnapshot::safe();
        let mut nml_error = invalid_nml_configuration;
        let result = unsafe { dmc2_task_status_poll(channel, &mut snapshot, &mut nml_error) };
        let transport_ok = result == 0 && nml_error == no_nml_error;
        let snapshot_ok = snapshot.valid_abi();
        unsafe {
            if transport_ok && snapshot_ok {
                let report = diagnostics::evaluate(&snapshot);
                publish_snapshot(
                    pins,
                    snapshot,
                    true,
                    false,
                    nml_error,
                    &report,
                    &mut diagnostic_state,
                );
            } else {
                let errors = ptr::read_volatile(pins.poll_errors).wrapping_add(1);
                ptr::write_volatile(pins.poll_errors, errors);
                let report = if transport_ok {
                    diagnostics::evaluate(&snapshot)
                } else {
                    diagnostics::disconnected(nml_error)
                };
                publish_snapshot(
                    pins,
                    NativeSnapshot::safe(),
                    false,
                    true,
                    nml_error,
                    &report,
                    &mut diagnostic_state,
                );
                dmc2_task_status_close(channel);
                channel = ptr::null_mut();
            }
        }
        thread::sleep(if transport_ok && snapshot_ok {
            POLL_PERIOD
        } else {
            RECONNECT_PERIOD
        });
    }

    #[allow(unreachable_code)]
    {
        unsafe {
            dmc2_task_status_close(channel);
            hal::hal_exit(_component_id);
        }
        Ok(())
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("dmc2-task-monitor: {error}");
        std::process::exit(1);
    }
}
