use dmc2_diagnostics::diagnostic_catalog;

pub const HEARTBEAT_TOGGLE_NS: u64 = 20_000_000;
pub const CONTROLLER_WATCHDOG_ARM_LOW_NS: u64 = 10_000_000;
pub const CONTROLLER_WATCHDOG_OK_WAIT_NS: u64 = 150_000_000;
pub const CONTROLLER_WATCHDOG_STABLE_NS: u64 = 250_000_000;
pub const MESA_WATCHDOG_STABLE_NS: u64 = 100_000_000;
pub const LIMIT_RESET_NS: u64 = 10_000_000;
pub const LIMIT_SETTLE_NS: u64 = 10_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeartbeatGenerator {
    value: bool,
    elapsed_ns: u64,
}

impl HeartbeatGenerator {
    pub const fn new() -> Self {
        Self {
            value: false,
            elapsed_ns: 0,
        }
    }

    pub fn update(&mut self, period_ns: u64) -> bool {
        self.elapsed_ns = self.elapsed_ns.saturating_add(period_ns);
        if self.elapsed_ns >= HEARTBEAT_TOGGLE_NS {
            // Toggle once and rebase. Never generate a catch-up burst after a
            // delayed cycle; the downstream watchdog must see the delay.
            self.value = !self.value;
            self.elapsed_ns = 0;
        }
        self.value
    }
}

impl Default for HeartbeatGenerator {
    fn default() -> Self {
        Self::new()
    }
}

diagnostic_catalog! {
    pub enum MesaStartupPhase {
        WaitServo = 0, "WAIT_SERVO", "wait-servo",
            "the Mesa startup guard is waiting for the LinuxCNC servo thread to become ready",
            "wait for the servo-thread-ready input or inspect LinuxCNC realtime startup";
        WatchdogStable = 1, "WATCHDOG_STABLE", "watchdog-stable",
            "the Mesa startup guard is clearing and observing the HostMot2 watchdog before enabling outputs",
            "wait for the watchdog-stable interval or inspect the named Mesa watchdog fault";
        LimitReset = 2, "LIMIT_RESET", "limit-reset",
            "the Mesa startup guard is asserting all configured realtime limit-latch resets",
            "wait for the bounded reset interval";
        LimitSettle = 3, "LIMIT_SETTLE", "limit-settle",
            "the Mesa startup guard is allowing the reset limit latches to settle",
            "wait for the bounded settle interval";
        Ready = 4, "READY", "ready",
            "the Mesa servo thread, watchdog, and startup limit-reset contract are ready",
            "no Mesa startup action is required";
        Fault = 5, "FAULT", "fault",
            "the Mesa startup guard latched an I/O or watchdog contract failure",
            "inspect the named controller fault and retained Mesa watchdog evidence";
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MesaStartupGuard {
    phase: MesaStartupPhase,
    phase_elapsed_ns: u64,
    pub watchdog_clear_requested: bool,
    pub limit_reset: [bool; 3],
    pub faulted: bool,
}

impl MesaStartupGuard {
    pub const fn new() -> Self {
        Self {
            phase: MesaStartupPhase::WaitServo,
            phase_elapsed_ns: 0,
            watchdog_clear_requested: false,
            limit_reset: [false; 3],
            faulted: false,
        }
    }

    pub const fn phase(&self) -> MesaStartupPhase {
        self.phase
    }

    pub const fn ready(&self) -> bool {
        matches!(self.phase, MesaStartupPhase::Ready) && !self.faulted
    }

    fn transition(&mut self, phase: MesaStartupPhase) {
        self.phase = phase;
        self.phase_elapsed_ns = 0;
    }

    fn fail(&mut self) {
        self.transition(MesaStartupPhase::Fault);
        self.watchdog_clear_requested = false;
        self.limit_reset = [false; 3];
        self.faulted = true;
    }

    pub fn update(
        &mut self,
        period_ns: u64,
        servo_thread_ready: bool,
        watchdog_has_bit: bool,
        io_error: bool,
    ) {
        self.watchdog_clear_requested = false;
        if self.faulted {
            return;
        }
        if io_error {
            self.fail();
            return;
        }
        if self.ready() {
            if watchdog_has_bit {
                self.fail();
            }
            return;
        }
        if matches!(self.phase, MesaStartupPhase::WaitServo) {
            if !servo_thread_ready {
                return;
            }
            self.transition(MesaStartupPhase::WatchdogStable);
        }
        if watchdog_has_bit {
            self.transition(MesaStartupPhase::WatchdogStable);
            self.limit_reset = [false; 3];
            self.watchdog_clear_requested = true;
            return;
        }

        self.phase_elapsed_ns = self.phase_elapsed_ns.saturating_add(period_ns);
        match self.phase {
            MesaStartupPhase::WatchdogStable
                if self.phase_elapsed_ns >= MESA_WATCHDOG_STABLE_NS =>
            {
                self.transition(MesaStartupPhase::LimitReset);
                self.limit_reset = [true; 3];
            }
            MesaStartupPhase::LimitReset if self.phase_elapsed_ns >= LIMIT_RESET_NS => {
                self.limit_reset = [false; 3];
                self.transition(MesaStartupPhase::LimitSettle);
            }
            MesaStartupPhase::LimitSettle if self.phase_elapsed_ns >= LIMIT_SETTLE_NS => {
                self.transition(MesaStartupPhase::Ready);
            }
            _ => {}
        }
    }
}

impl Default for MesaStartupGuard {
    fn default() -> Self {
        Self::new()
    }
}

diagnostic_catalog! {
    pub enum ControllerWatchdogPhase {
        WaitPrerequisites = 0, "WAIT_PREREQUISITES", "wait-prerequisites",
            "the controller heartbeat watchdog is waiting for every startup prerequisite",
            "inspect the named readiness inputs if this phase persists";
        ArmLow = 1, "ARM_LOW", "arm-low",
            "the controller is holding watchdog-enable low for the required rearm interval",
            "wait for the bounded low interval";
        WaitOk = 2, "WAIT_OK", "wait-ok",
            "the controller enabled the software watchdog and is waiting for its healthy acknowledgement",
            "inspect the watchdog component and heartbeat signal if acknowledgement does not arrive";
        Stable = 3, "STABLE", "stable",
            "the watchdog acknowledgement is healthy and being observed for the required stable interval",
            "wait for the bounded stable interval";
        Ready = 4, "READY", "ready",
            "the controller heartbeat watchdog contract is armed and healthy",
            "no watchdog startup action is required";
        Fault = 5, "FAULT", "fault",
            "the committed controller watchdog contract lost its healthy acknowledgement",
            "inspect the named watchdog fault and retained heartbeat evidence before resetting";
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControllerWatchdogGuard {
    phase: ControllerWatchdogPhase,
    phase_elapsed_ns: u64,
    pub enable: bool,
    pub runtime_committed: bool,
    pub faulted: bool,
    pub arm_attempts: u32,
}

impl ControllerWatchdogGuard {
    pub const fn new() -> Self {
        Self {
            phase: ControllerWatchdogPhase::WaitPrerequisites,
            phase_elapsed_ns: 0,
            enable: false,
            runtime_committed: false,
            faulted: false,
            arm_attempts: 0,
        }
    }

    pub const fn phase(&self) -> ControllerWatchdogPhase {
        self.phase
    }

    pub const fn ready(&self) -> bool {
        matches!(self.phase, ControllerWatchdogPhase::Ready) && !self.faulted
    }

    pub fn commit_runtime(&mut self) -> bool {
        if !self.ready() {
            return false;
        }
        self.runtime_committed = true;
        true
    }

    fn transition(&mut self, phase: ControllerWatchdogPhase) {
        self.phase = phase;
        self.phase_elapsed_ns = 0;
    }

    fn wait_for_prerequisites(&mut self) {
        self.transition(ControllerWatchdogPhase::WaitPrerequisites);
        self.enable = false;
    }

    fn begin_low(&mut self) {
        self.transition(ControllerWatchdogPhase::ArmLow);
        self.enable = false;
    }

    fn fail(&mut self) {
        self.transition(ControllerWatchdogPhase::Fault);
        self.enable = false;
        self.faulted = true;
    }

    pub fn update(&mut self, period_ns: u64, watchdog_ok: bool, prerequisites_ready: bool) {
        if self.faulted {
            return;
        }
        if self.ready() {
            if !self.runtime_committed && !prerequisites_ready {
                self.wait_for_prerequisites();
            } else if !watchdog_ok {
                if self.runtime_committed {
                    self.fail();
                } else {
                    self.begin_low();
                }
            }
            return;
        }
        if !prerequisites_ready {
            if !matches!(self.phase, ControllerWatchdogPhase::WaitPrerequisites) {
                self.wait_for_prerequisites();
            }
            return;
        }

        self.phase_elapsed_ns = self.phase_elapsed_ns.saturating_add(period_ns);
        match self.phase {
            ControllerWatchdogPhase::WaitPrerequisites => self.begin_low(),
            ControllerWatchdogPhase::ArmLow
                if self.phase_elapsed_ns >= CONTROLLER_WATCHDOG_ARM_LOW_NS =>
            {
                self.enable = true;
                self.arm_attempts = self.arm_attempts.wrapping_add(1);
                self.transition(ControllerWatchdogPhase::WaitOk);
            }
            ControllerWatchdogPhase::WaitOk if watchdog_ok => {
                self.transition(ControllerWatchdogPhase::Stable);
            }
            ControllerWatchdogPhase::WaitOk
                if self.phase_elapsed_ns >= CONTROLLER_WATCHDOG_OK_WAIT_NS =>
            {
                self.begin_low();
            }
            ControllerWatchdogPhase::Stable if !watchdog_ok => self.begin_low(),
            ControllerWatchdogPhase::Stable
                if self.phase_elapsed_ns >= CONTROLLER_WATCHDOG_STABLE_NS =>
            {
                self.transition(ControllerWatchdogPhase::Ready);
            }
            _ => {}
        }
    }
}

impl Default for ControllerWatchdogGuard {
    fn default() -> Self {
        Self::new()
    }
}
