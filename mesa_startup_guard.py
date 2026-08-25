from __future__ import annotations


PHASE_WAIT_SERVO = "mesa-wait-servo"
PHASE_WATCHDOG_STABLE = "mesa-watchdog-stable"
PHASE_LIMIT_RESET = "mesa-limit-reset"
PHASE_LIMIT_SETTLE = "mesa-limit-settle"
PHASE_READY = "mesa-ready"
PHASE_FAULT = "mesa-fault"

WATCHDOG_STABLE_SECONDS = 0.100
LIMIT_RESET_SECONDS = 0.010
LIMIT_SETTLE_SECONDS = 0.010


class MesaStartupGuard:
    """Bring a previously stopped HostMot2 board to a valid read state.

    The HostMot2 watchdog leaves all card pins pulled high after a prior
    controller exits. During startup only, this guard requests that the
    bidirectional ``watchdog.has_bit`` HAL signal be cleared, waits for it to
    remain healthy, clears the limit-event latches, and then releases the main
    pendant supervisor. A watchdog bite after readiness is a fault and is
    never automatically cleared.
    """

    def __init__(self) -> None:
        self.phase = PHASE_WAIT_SERVO
        self.watchdog_stable_since: float | None = None
        self.phase_deadline: float | None = None
        self.watchdog_clear_requested = False
        self.limit_reset = (False, False, False)
        self.ready = False
        self.fault: str | None = None

    @property
    def faulted(self) -> bool:
        return self.fault is not None

    def _fail(self, reason: str) -> None:
        if self.fault is not None:
            return
        self.phase = PHASE_FAULT
        self.watchdog_clear_requested = False
        self.limit_reset = (False, False, False)
        self.ready = False
        self.fault = reason

    def update(
        self,
        *,
        now: float,
        servo_thread_ready: bool,
        watchdog_has_bit: bool,
        io_error: bool = False,
    ) -> None:
        self.watchdog_clear_requested = False

        if self.faulted:
            return

        if io_error:
            location = "after startup" if self.ready else "during startup"
            self._fail(f"Mesa low-level I/O error {location}")
            return

        if self.ready:
            if watchdog_has_bit:
                self._fail("Mesa watchdog bit after startup")
            return

        if self.phase == PHASE_WAIT_SERVO:
            if not servo_thread_ready:
                return
            self.phase = PHASE_WATCHDOG_STABLE
            self.watchdog_stable_since = None

        if watchdog_has_bit:
            self.phase = PHASE_WATCHDOG_STABLE
            self.watchdog_stable_since = None
            self.phase_deadline = None
            self.limit_reset = (False, False, False)
            self.watchdog_clear_requested = True
            return

        if self.phase == PHASE_WATCHDOG_STABLE:
            if self.watchdog_stable_since is None:
                self.watchdog_stable_since = now
                return
            if now - self.watchdog_stable_since < WATCHDOG_STABLE_SECONDS:
                return
            self.phase = PHASE_LIMIT_RESET
            self.limit_reset = (True, True, True)
            self.phase_deadline = now + LIMIT_RESET_SECONDS
            return

        if self.phase == PHASE_LIMIT_RESET:
            if self.phase_deadline is None or now < self.phase_deadline:
                return
            self.limit_reset = (False, False, False)
            self.phase = PHASE_LIMIT_SETTLE
            self.phase_deadline = now + LIMIT_SETTLE_SECONDS
            return

        if self.phase == PHASE_LIMIT_SETTLE:
            if self.phase_deadline is None or now < self.phase_deadline:
                return
            self.phase = PHASE_READY
            self.phase_deadline = None
            self.ready = True
