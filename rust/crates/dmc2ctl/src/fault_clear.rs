//! One explicit operator Clear Fault request. Never run from a poll/watchdog.
use std::fmt;
use std::thread;
use std::time::{Duration, Instant};

use dmc2_core::startup::MESA_WATCHDOG_STABLE_NS;
use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};

use crate::dispatch::STATUS_POLL_PERIOD;
use crate::hal::{self, DriverReset, HalError};
use crate::native::{ControlBackend, MachineState, NativeError, Receipt};

// Same operator-command deadline as native/control_client.cc COMMAND_TIMEOUT.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const MESA_STABLE: Duration = Duration::from_nanos(MESA_WATCHDOG_STABLE_NS);

#[derive(Clone, Copy, Debug)]
struct MesaState {
    io_error: bool,
    packet_error: bool,
    packet_error_exceeded: bool,
    watchdog: bool,
}

impl MesaState {
    fn read() -> Result<Self, FaultClearError> {
        Ok(Self {
            io_error: hal::read_bit("hm2_7i95.0.io_error")?,
            packet_error: hal::read_bit("hm2_7i95.0.packet-error")?,
            packet_error_exceeded: hal::read_bit("hm2_7i95.0.packet-error-exceeded")?,
            watchdog: hal::read_bit("hm2_7i95.0.watchdog.has_bit")?,
        })
    }

    fn healthy(self) -> bool {
        !self.io_error && !self.packet_error && !self.packet_error_exceeded && !self.watchdog
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RecoveryStage {
    DriverCommunication,
    ControllerAcknowledgement,
}

fn require_stopped(
    backend: &mut impl ControlBackend,
    expected_command: i32,
) -> Result<(), FaultClearError> {
    let status = backend.status()?;
    if status.echo_serial_number != expected_command {
        return Err(FaultClearError::Superseded);
    }
    if status.machine_state != MachineState::Estop
        || !status.interpreter_idle()
        || status.homing_mask != 0
        || status.spindle_speed != 0.0
        || status.spindle_direction != 0
        || hal::read_bit("motion.motion-enabled")?
        || hal::read_bit("dmc2-pendant-control.external-enable")?
    {
        return Err(FaultClearError::MustStop);
    }
    require_released_estop()
}

fn require_released_estop() -> Result<(), FaultClearError> {
    if hal::read_bit(hal::PHYSICAL_PENDANT_ESTOP_PIN)? {
        return Err(FaultClearError::PhysicalEstopPressed);
    }
    Ok(())
}

fn recover_driver(
    backend: &mut impl ControlBackend,
    initial: MesaState,
) -> Result<(), FaultClearError> {
    let expected_command = backend.status()?.echo_serial_number;
    require_stopped(backend, expected_command)?;
    if initial.io_error {
        // LinuxCNC 2.9.10 hm2_eth.c: record_soft_error latches llio.io_error.
        // receive_queued_reads resets its accumulator after this manual clear.
        hal::acknowledge_driver(DriverReset::MesaIoError)?;
        println!("CLEAR_FAULT driver=mesa action=acknowledge-retained-io-error");
    }
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    let mut stable_since = None;
    let mut watchdog_acknowledged = false;
    loop {
        require_stopped(backend, expected_command)?;
        let state = MesaState::read()?;
        // A failed attempt never keeps resetting a bad connection.
        if state.io_error {
            return Err(FaultClearError::DriverFaultReturned);
        }
        if state.watchdog && !watchdog_acknowledged {
            hal::acknowledge_driver(DriverReset::MesaWatchdog)?;
            watchdog_acknowledged = true;
            stable_since = None;
            println!("CLEAR_FAULT driver=mesa action=acknowledge-watchdog");
        } else if state.healthy() {
            let since = stable_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= MESA_STABLE {
                require_stopped(backend, expected_command)?;
                return Ok(());
            }
        } else {
            stable_since = None;
        }
        if Instant::now() >= deadline {
            return Err(FaultClearError::Timeout(RecoveryStage::DriverCommunication));
        }
        thread::sleep(STATUS_POLL_PERIOD);
    }
}

pub fn execute(backend: &mut impl ControlBackend) -> Result<Receipt, FaultClearError> {
    let mesa = MesaState::read()?;
    if mesa.io_error || mesa.packet_error_exceeded || mesa.watchdog {
        recover_driver(backend, mesa)?;
    }
    require_released_estop()?;
    // Preserve the canonical LinuxCNC request consumed by the realtime
    // controller, tool setter, and existing manual recovery paths.
    // No Machine On, mode change, resume, homing, or motion is issued here.
    let receipt = backend.set_state(MachineState::EstopReset)?;
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    loop {
        let status = backend.status()?;
        require_released_estop()?;
        let mesa = MesaState::read()?;
        if mesa.io_error || mesa.packet_error_exceeded || mesa.watchdog {
            return Err(FaultClearError::DriverFaultReturned);
        }
        if !hal::read_bit("dmc2-pendant-control.fault")?
            && status.machine_state != MachineState::Estop
            && !status.auxiliary_estop
        {
            println!("CLEAR_FAULT observed=controller-latch-clear machine_state={}; Machine On and Pendant Mode remain operator controls", status.machine_state.name());
            return Ok(receipt);
        }
        if Instant::now() >= deadline {
            return Err(FaultClearError::Timeout(
                RecoveryStage::ControllerAcknowledgement,
            ));
        }
        thread::sleep(STATUS_POLL_PERIOD);
    }
}

#[derive(Debug)]
pub enum FaultClearError {
    Hal(HalError),
    Native(NativeError),
    MustStop,
    Superseded,
    PhysicalEstopPressed,
    DriverFaultReturned,
    Timeout(RecoveryStage),
}

impl From<HalError> for FaultClearError {
    fn from(error: HalError) -> Self {
        Self::Hal(error)
    }
}
impl From<NativeError> for FaultClearError {
    fn from(error: NativeError) -> Self {
        Self::Native(error)
    }
}
impl fmt::Display for FaultClearError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hal(error) => error.fmt(f),
            Self::Native(error) => error.fmt(f),
            Self::MustStop => f.write_str("Mesa recovery needs LinuxCNC in E-stop, idle, not homing, with spindle and motion commands disabled. Use the visible Abort and E-stop controls, then retry Clear Fault."),
            Self::Superseded => f.write_str("A newer LinuxCNC command superseded this Clear Fault request. No further reset was submitted. Press Clear Fault again when ready."),
            Self::PhysicalEstopPressed => f.write_str("The physical pendant E-stop is pressed. Release it, then press Clear Fault again."),
            Self::DriverFaultReturned => f.write_str("Mesa communication or watchdog fault returned during Clear Fault. The controller remains blocked. Restore the Mesa connection, then retry Clear Fault; this attempt will not reset it again."),
            Self::Timeout(RecoveryStage::DriverCommunication) => f.write_str("Mesa did not report stable communication after the requested reset. Restore the Mesa connection, then retry Clear Fault. The controller latch was retained."),
            Self::Timeout(RecoveryStage::ControllerAcknowledgement) => f.write_str("LinuxCNC has not acknowledged controller recovery. The retained fault details remain in the display. Release any named active input, use Abort if needed, and retry Clear Fault; Pendant Mode remains accessible."),
        }
    }
}
impl RecoveryClassified for FaultClearError {
    fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::Hal(error) => error.recovery_class(),
            Self::Native(error) => error.recovery_class(),
            Self::MustStop | Self::PhysicalEstopPressed => RecoveryClass::RestoreMachine,
            Self::DriverFaultReturned | Self::Timeout(_) | Self::Superseded => {
                RecoveryClass::ClearController
            }
        }
    }
}
