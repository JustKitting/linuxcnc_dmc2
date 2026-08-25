//! Safety supervisor for pendant-driven LinuxCNC motion.
//!
//! The submodules are separated by responsibility: public input/output
//! contracts, stable fault codes, HostMot2 feedback validation, and the state
//! machine that coordinates those pieces.

mod controller;
mod fault;
mod feedback;
mod model;

pub use controller::{
    LinuxCncPendantSupervisor, Phase, BOUNCE_RATE_PULSES_PER_SECOND, BOUNCE_SPEED_MM_PER_MINUTE,
    BOUNCE_TIMEOUT_NS, GATE_SETTLE_NS, JOG_TARGET_TOLERANCE_PULSES, LIMIT_RESET_NS,
    LIMIT_RESET_TIMEOUT_NS, LIMIT_RESET_VALIDATE_NS, MOTION_SETTLE_NS,
};
pub use fault::FaultCode;
pub use model::{
    CommandEvent, JogCommand, LinkSnapshot, MachineSnapshot, SupervisorInputs, SupervisorOutputs,
};
