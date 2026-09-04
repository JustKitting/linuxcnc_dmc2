//! LinuxCNC realtime component lifecycle and exported C ABI.

mod cycle;
mod hal;
mod state;

use core::ffi::{c_char, c_int, c_long, c_void};
use core::fmt::{self, Write};
use core::{mem, ptr};

use dmc2_diagnostics::{RecoveryClass, RecoveryClassified, RecoveryDisplay};
use dmc2_hal_sys as linuxcnc_hal;

use self::cycle::update_component;
use self::hal::{publish_initial_safe, register_pins, Pins};
use self::state::ComponentState;

// `loadrt dmc2_rt` waits for this exact HAL component name. Pin and function
// names intentionally retain the stable `dmc2-pendant-control` namespace.
const COMPONENT_NAME: &[u8] = b"dmc2_rt\0";
const FUNCTION_NAME: &[u8] = b"dmc2-pendant-control.update\0";
const _: () = assert!(COMPONENT_NAME.len() - 1 <= linuxcnc_hal::HAL_NAME_LEN as usize);
const _: () = assert!(FUNCTION_NAME.len() - 1 <= linuxcnc_hal::HAL_NAME_LEN as usize);
const RTAPI_MESSAGE_FORMAT: &[u8] = b"dmc2_rt: %s\n\0";
const RECOVERY_RENDER_FAILURE: &[u8] = b"dmc2_rt: DMC2_RT_RECOVERY_RENDER_FAILED: the typed startup failure exceeded the retained RTAPI message buffer; action: inspect the retained startup source and relaunch DMC2 LinuxCNC; recovery-class=RELAUNCH_APPLICATION; recovery-transition=APPLICATION_RELAUNCHED; clear-condition=correct the named installation or runtime cause and launch a matched DMC2 LinuxCNC session; ui-path=application.launch -> controller.clear-fault -> controller.pendant-mode\n\0";
const RTAPI_MESSAGE_CAPACITY: usize = 2048;
const ENOMEM: c_int = linuxcnc_hal::HalKnownErrno::OutOfMemory.raw();

static mut COMPONENT_ID: c_int = -1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StartupFailure {
    Hal(linuxcnc_hal::HalError),
    PinStorageAllocation,
    StateAllocation,
}

impl StartupFailure {
    const fn safe_return_code(self) -> c_int {
        match self {
            Self::Hal(error) => error.safe_return_code(),
            Self::PinStorageAllocation | Self::StateAllocation => ENOMEM,
        }
    }
}

impl fmt::Display for StartupFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hal(error) => error.fmt(formatter),
            Self::PinStorageAllocation => formatter.write_str(
                "HAL_PIN_STORAGE_ALLOCATION_FAILED (raw=null, call=hal_malloc): HAL shared memory could not hold the realtime pin-pointer structure; action: stop duplicate HAL owners and restore sufficient HAL shared memory before restarting",
            ),
            Self::StateAllocation => formatter.write_str(
                "HAL_COMPONENT_STATE_ALLOCATION_FAILED (raw=null, call=hal_malloc): HAL shared memory could not hold the realtime controller state; action: stop duplicate HAL owners and restore sufficient HAL shared memory before restarting",
            ),
        }
    }
}

impl RecoveryClassified for StartupFailure {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::Hal(error) => error.recovery_class(),
            Self::PinStorageAllocation | Self::StateAllocation => {
                RecoveryClass::RelaunchApplication
            }
        }
    }
}

impl From<linuxcnc_hal::HalError> for StartupFailure {
    fn from(error: linuxcnc_hal::HalError) -> Self {
        Self::Hal(error)
    }
}

unsafe fn exit_component(component_id: c_int) {
    if let Err(error) =
        linuxcnc_hal::HalCall::Exit.classify(unsafe { linuxcnc_hal::hal_exit(component_id) })
    {
        unsafe { log_startup_failure(StartupFailure::Hal(error)) };
    }
}

struct RtapiMessage {
    bytes: [u8; RTAPI_MESSAGE_CAPACITY],
    length: usize,
}

impl RtapiMessage {
    const fn new() -> Self {
        Self {
            bytes: [0; RTAPI_MESSAGE_CAPACITY],
            length: 0,
        }
    }

    const fn as_ptr(&self) -> *const c_char {
        self.bytes.as_ptr().cast::<c_char>()
    }
}

impl Write for RtapiMessage {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let remaining = self
            .bytes
            .len()
            .saturating_sub(self.length)
            .saturating_sub(1);
        if value.len() > remaining {
            return Err(fmt::Error);
        }
        let end = self.length + value.len();
        self.bytes[self.length..end].copy_from_slice(value.as_bytes());
        self.length = end;
        self.bytes[self.length] = 0;
        Ok(())
    }
}

unsafe fn log_startup_failure(error: StartupFailure) {
    let mut message = RtapiMessage::new();
    if write!(&mut message, "{}", RecoveryDisplay(&error)).is_err() {
        unsafe {
            linuxcnc_hal::rtapi_print_msg(
                linuxcnc_hal::msg_level_t_RTAPI_MSG_ERR,
                RECOVERY_RENDER_FAILURE.as_ptr().cast::<c_char>(),
            );
        }
        return;
    }
    unsafe {
        linuxcnc_hal::rtapi_print_msg(
            linuxcnc_hal::msg_level_t_RTAPI_MSG_ERR,
            RTAPI_MESSAGE_FORMAT.as_ptr().cast::<c_char>(),
            message.as_ptr(),
        );
    }
}

#[cfg(not(debug_assertions))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    unsafe extern "C" {
        fn abort() -> !;
    }
    unsafe { abort() }
}

#[no_mangle]
pub extern "C" fn rtapi_app_main() -> c_int {
    let component_id = match linuxcnc_hal::HalCall::Init
        .classify(unsafe { linuxcnc_hal::hal_init(COMPONENT_NAME.as_ptr().cast::<c_char>()) })
    {
        Ok(component_id) => component_id,
        Err(error) => {
            unsafe { log_startup_failure(StartupFailure::Hal(error)) };
            return error.safe_return_code();
        }
    };
    unsafe { COMPONENT_ID = component_id };

    let result = (|| -> Result<(), StartupFailure> {
        let pins =
            unsafe { linuxcnc_hal::hal_malloc(mem::size_of::<Pins>() as c_long) }.cast::<Pins>();
        if pins.is_null() {
            return Err(StartupFailure::PinStorageAllocation);
        }
        unsafe {
            ptr::write_bytes(pins, 0, 1);
            register_pins(pins, component_id)?;
            publish_initial_safe(&*pins);
        }

        let state = unsafe { linuxcnc_hal::hal_malloc(mem::size_of::<ComponentState>() as c_long) }
            .cast::<ComponentState>();
        if state.is_null() {
            return Err(StartupFailure::StateAllocation);
        }
        unsafe { ptr::write(state, ComponentState::new(pins)) };

        let exported = unsafe {
            linuxcnc_hal::hal_export_funct(
                FUNCTION_NAME.as_ptr().cast::<c_char>(),
                Some(update_component),
                state.cast::<c_void>(),
                1,
                0,
                component_id,
            )
        };
        linuxcnc_hal::HalCall::ExportFunct.classify(exported)?;
        linuxcnc_hal::HalCall::Ready.classify(unsafe { linuxcnc_hal::hal_ready(component_id) })?;
        Ok(())
    })();

    match result {
        Ok(()) => 0,
        Err(error) => {
            unsafe {
                log_startup_failure(error);
                exit_component(component_id);
                COMPONENT_ID = -1;
            }
            error.safe_return_code()
        }
    }
}

#[no_mangle]
pub extern "C" fn rtapi_app_exit() {
    let component_id = unsafe { COMPONENT_ID };
    if component_id >= 0 {
        unsafe {
            exit_component(component_id);
            COMPONENT_ID = -1;
        }
    }
}
