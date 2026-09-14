//! One explicit operator Clear Fault request. Never run from a poll/watchdog.
use std::fmt;
use std::thread;
use std::time::{Duration, Instant};

use dmc2_core::startup::MESA_WATCHDOG_STABLE_NS;
use dmc2_diagnostics::{RecoveryClass, RecoveryClassified};

use crate::dispatch::STATUS_POLL_PERIOD;
use crate::hal::{self, DriverReset, HalError, COMMAND_TIMEOUT};
use crate::native::{ControlBackend, MachineState, NativeError, Receipt};

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
pub struct ClearRequest(u32);

impl ClearRequest {
    fn current(self) -> Result<(), FaultClearError> {
        if hal::read_u32(hal::CLEAR_REQUEST_PIN)? != self.0 {
            return Err(FaultClearError::Superseded);
        }
        Ok(())
    }

    fn await_ack(self) -> Result<(), FaultClearError> {
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        loop {
            self.current()?;
            if hal::read_u32(hal::CLEAR_ACK_PIN)? == self.0 {
                println!(
                    "CLEAR_FAULT request={} observed=realtime-clear-acknowledged",
                    self.0
                );
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(FaultClearError::Timeout(
                    RecoveryStage::RealtimeAcknowledgement,
                ));
            }
            thread::sleep(STATUS_POLL_PERIOD);
        }
    }
}

pub fn submit() -> Result<ClearRequest, FaultClearError> {
    let request = ClearRequest(hal::submit_clear()?);
    println!(
        "CLEAR_FAULT request={} submitted=operator-priority-clear",
        request.0
    );
    Ok(request)
}

#[derive(Clone, Copy, Debug)]
pub enum RecoveryStage {
    RealtimeAcknowledgement,
    DriverCommunication,
    ControllerAcknowledgement,
}

fn recover_driver(request: ClearRequest, initial: MesaState) -> Result<(), FaultClearError> {
    request.current()?;
    if initial.io_error {
        // LinuxCNC 2.9.10 hm2_eth.c resets its accumulator on this manual ack.
        hal::acknowledge_driver(DriverReset::MesaIoError)?;
        println!("CLEAR_FAULT driver=mesa action=acknowledge-retained-io-error");
    }
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    let mut stable_since = None;
    let mut watchdog_acknowledged = false;
    loop {
        request.current()?;
        let state = MesaState::read()?;
        // Report a returning cause. Never repeatedly reset a failed connection.
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
    let request = submit()?;
    execute_requested(backend, request)
}

pub fn execute_requested(
    backend: &mut impl ControlBackend,
    request: ClearRequest,
) -> Result<Receipt, FaultClearError> {
    // Recovery causes the required stop; it never demands that a faulted task
    // first become idle or lose its retained HOME_ABORT/homing flags.
    request.current()?;
    let abort = backend.abort();
    let stop = backend.set_state(MachineState::Estop);
    // An Abort error cannot suppress submission of the E-stop command.
    if let Err(error) = abort {
        eprintln!("OPERATOR_WARNING=Abort did not acknowledge: {error}. Clear Fault also submitted E-stop and is continuing recovery.");
    }
    let stop = stop?;
    request.await_ack()?;
    let mesa = MesaState::read()?;
    if !mesa.healthy() {
        recover_driver(request, mesa)?;
    }
    request.current()?;
    if hal::read_bit(hal::PHYSICAL_PENDANT_ESTOP_PIN)? {
        println!("OPERATOR_MESSAGE=Clear Fault was processed. The physical pendant E-stop remains pressed; release it and press Clear Fault to release E-stop. Pendant Mode remains selectable.");
        return Ok(stop);
    }
    // This canonical reset also serves the existing pendant and setter paths.
    // Machine On, resume, homing and motion remain separate operator commands.
    let receipt = backend.set_state(MachineState::EstopReset)?;
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    loop {
        request.current()?;
        let status = backend.status()?;
        let mesa = MesaState::read()?;
        if mesa.io_error || mesa.packet_error_exceeded || mesa.watchdog {
            return Err(FaultClearError::DriverFaultReturned);
        }
        if hal::read_bit(hal::PHYSICAL_PENDANT_ESTOP_PIN)? {
            println!("OPERATOR_MESSAGE=Clear Fault was processed. The physical pendant E-stop is pressed; release it and use Clear Fault again. Pendant Mode remains selectable.");
            return Ok(receipt);
        }
        if !hal::read_bit("dmc2-pendant-control.fault")?
            && status.machine_state != MachineState::Estop
            && !status.auxiliary_estop
        {
            println!("OPERATOR_MESSAGE=Clear Fault was acknowledged and LinuxCNC reports E-stop reset. Use Machine On and Pendant Mode to resume manual control.");
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
    Superseded,
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
            Self::Native(error) => write!(f, "Clear Fault was submitted to the realtime controller, but LinuxCNC task communication reported: {error}. Retry Clear Fault; use the visible CNC launcher to reopen the session if task communication remains unavailable."),
            Self::Superseded => f.write_str("A newer Clear Fault request has replaced this attempt. The newer request now owns recovery."),
            Self::DriverFaultReturned => f.write_str("Clear Fault was acknowledged, but Mesa reports a current communication or watchdog fault. Restore the Mesa connection and press Clear Fault again. The UI recovery controls remain available."),
            Self::Timeout(RecoveryStage::RealtimeAcknowledgement) => f.write_str("The realtime controller has not acknowledged Clear Fault. LinuxCNC E-stop was submitted. Retry Clear Fault; if the realtime component remains unavailable, reopen the session with the visible CNC launcher."),
            Self::Timeout(RecoveryStage::DriverCommunication) => f.write_str("Clear Fault was acknowledged, but Mesa has not reported stable communication after its acknowledgement. Restore the Mesa connection and press Clear Fault again."),
            Self::Timeout(RecoveryStage::ControllerAcknowledgement) => f.write_str("The realtime controller accepted Clear Fault, but LinuxCNC has not reported E-stop reset. The display retains the current cause. Release any named active input and press Clear Fault again; Pendant Mode remains selectable."),
        }
    }
}
impl RecoveryClassified for FaultClearError {
    fn recovery_class(&self) -> RecoveryClass {
        RecoveryClass::ClearController
    }
}
