use std::env;
use std::ffi::{c_char, c_int, CString};
use std::mem;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

use dmc2_hal_sys as hal;

const DEFAULT_COMPONENT: &str = "dmc2-task-monitor";
const DEFAULT_NML_FILE: &str = "/usr/share/linuxcnc/linuxcnc.nml";
const POLL_PERIOD: Duration = Duration::from_millis(10);
const RECONNECT_PERIOD: Duration = Duration::from_secs(1);

#[repr(C)]
struct TaskStatusChannel {
    _private: [u8; 0],
}

#[derive(Clone, Copy)]
#[repr(C)]
struct NativeSnapshot {
    heartbeat: u32,
    machine_on: u32,
    estopped: u32,
    manual_mode: u32,
    joint_mode: u32,
    teleop_mode: u32,
    interp_idle: u32,
    homed: [u32; 3],
    homing: [u32; 3],
    axis_stopped: [u32; 3],
}

impl NativeSnapshot {
    const fn safe() -> Self {
        Self {
            heartbeat: 0,
            machine_on: 0,
            estopped: 1,
            manual_mode: 0,
            joint_mode: 0,
            teleop_mode: 0,
            interp_idle: 0,
            homed: [0; 3],
            homing: [0; 3],
            axis_stopped: [1; 3],
        }
    }
}

unsafe extern "C" {
    fn dmc2_task_status_open(nml_file: *const c_char) -> *mut TaskStatusChannel;
    fn dmc2_task_status_poll(
        channel: *mut TaskStatusChannel,
        snapshot: *mut NativeSnapshot,
    ) -> c_int;
    fn dmc2_task_status_close(channel: *mut TaskStatusChannel);
}

#[derive(Debug)]
struct Arguments {
    component: String,
    nml_file: String,
}

fn arguments() -> Result<Arguments, String> {
    let mut component = DEFAULT_COMPONENT.to_owned();
    let mut nml_file = env::var("EMC2_NMLFILE").unwrap_or_else(|_| DEFAULT_NML_FILE.to_owned());
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
            "--help" | "-h" => {
                println!("Usage: dmc2-task-monitor [--component NAME] [--nml-file PATH]");
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(Arguments {
        component,
        nml_file,
    })
}

struct HalPins {
    snapshot_generation: *mut hal::hal_u32_t,
    connected: *mut hal::hal_bit_t,
    fault: *mut hal::hal_bit_t,
    task_heartbeat: *mut hal::hal_u32_t,
    publications: *mut hal::hal_u32_t,
    poll_errors: *mut hal::hal_u32_t,
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

unsafe fn new_bit_pin(
    component: &str,
    suffix: &str,
    pointer: *mut *mut hal::hal_bit_t,
    component_id: c_int,
) -> Result<(), String> {
    let name = CString::new(format!("{component}.{suffix}"))
        .map_err(|_| "HAL pin name contained a NUL byte".to_owned())?;
    let result = unsafe {
        hal::hal_pin_bit_new(
            name.as_ptr(),
            hal::hal_pin_dir_t_HAL_OUT,
            pointer,
            component_id,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(format!("hal_pin_bit_new({suffix}) failed: {result}"))
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

unsafe fn publish_snapshot(pins: &HalPins, snapshot: NativeSnapshot, connected: bool, fault: bool) {
    let publications = unsafe { ptr::read_volatile(pins.publications) }.wrapping_add(1);
    let generation = publications.wrapping_shl(1);
    let generation_pin = unsafe { &*(pins.snapshot_generation.cast::<AtomicU32>()) };
    unsafe {
        generation_pin.store(generation | 1, Ordering::SeqCst);
        ptr::write_volatile(pins.task_heartbeat, snapshot.heartbeat);
        ptr::write_volatile(pins.machine_on, snapshot.machine_on != 0);
        ptr::write_volatile(pins.estopped, snapshot.estopped != 0);
        ptr::write_volatile(pins.manual_mode, snapshot.manual_mode != 0);
        ptr::write_volatile(pins.joint_mode, snapshot.joint_mode != 0);
        ptr::write_volatile(pins.teleop_mode, snapshot.teleop_mode != 0);
        ptr::write_volatile(pins.interp_idle, snapshot.interp_idle != 0);
        for index in 0..3 {
            ptr::write_volatile(pins.homed[index], snapshot.homed[index] != 0);
            ptr::write_volatile(pins.homing[index], snapshot.homing[index] != 0);
            ptr::write_volatile(pins.axis_stopped[index], snapshot.axis_stopped[index] != 0);
        }
        ptr::write_volatile(pins.connected, connected);
        ptr::write_volatile(pins.fault, fault);
        ptr::write_volatile(pins.publications, publications);
        generation_pin.store(generation, Ordering::SeqCst);
    }
}

fn run() -> Result<(), String> {
    let args = arguments()?;
    let nml_file = CString::new(args.nml_file.as_str())
        .map_err(|_| "NML file path contained a NUL byte".to_owned())?;
    let (_component_id, pins_pointer) = unsafe { create_hal(&args.component)? };
    let pins = unsafe { &*pins_pointer };

    let mut channel: *mut TaskStatusChannel = ptr::null_mut();

    loop {
        if channel.is_null() {
            channel = unsafe { dmc2_task_status_open(nml_file.as_ptr()) };
            if channel.is_null() {
                unsafe {
                    let errors = ptr::read_volatile(pins.poll_errors).wrapping_add(1);
                    ptr::write_volatile(pins.poll_errors, errors);
                    publish_snapshot(pins, NativeSnapshot::safe(), false, true);
                }
                thread::sleep(RECONNECT_PERIOD);
                continue;
            }
        }
        let mut snapshot = NativeSnapshot::safe();
        let result = unsafe { dmc2_task_status_poll(channel, &mut snapshot) };
        unsafe {
            if result == 0 {
                publish_snapshot(pins, snapshot, true, false);
            } else {
                let errors = ptr::read_volatile(pins.poll_errors).wrapping_add(1);
                ptr::write_volatile(pins.poll_errors, errors);
                publish_snapshot(pins, NativeSnapshot::safe(), false, true);
                dmc2_task_status_close(channel);
                channel = ptr::null_mut();
            }
        }
        thread::sleep(if result == 0 {
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
