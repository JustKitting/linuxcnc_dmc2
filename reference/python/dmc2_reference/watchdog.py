"""Legacy Python oracle for controller-watchdog startup policy."""

from __future__ import annotations


PHASE_WAIT_PREREQUISITES = "controller-watchdog-wait-prerequisites"
PHASE_ARM_LOW = "controller-watchdog-arm-low"
PHASE_WAIT_OK = "controller-watchdog-wait-ok"
PHASE_STABLE = "controller-watchdog-stable"
PHASE_READY = "controller-watchdog-ready"
PHASE_FAULT = "controller-watchdog-fault"

HEARTBEAT_TOGGLE_SECONDS = 0.020
WATCHDOG_ARM_LOW_SECONDS = 0.010
WATCHDOG_OK_WAIT_SECONDS = 0.150
WATCHDOG_STABLE_SECONDS = 0.250


class HeartbeatGenerator:
    """Generate a level that persists across many 1 kHz servo samples."""

    def __init__(self, *, toggle_seconds: float = HEARTBEAT_TOGGLE_SECONDS) -> None:
        if toggle_seconds <= 0:
            raise ValueError("heartbeat toggle period must be positive")
        self.toggle_seconds = toggle_seconds
        self.value = False
        self.next_toggle_at: float | None = None

    def update(self, now: float) -> bool:
        if self.next_toggle_at is None:
            self.next_toggle_at = now + self.toggle_seconds
            return self.value
        if now >= self.next_toggle_at:
            # Toggle once and rebase from the actual update. Never emit a
            # catch-up burst after a scheduling delay; the realtime watchdog
            # must be allowed to detect a genuinely late userspace process.
            self.value = not self.value
            self.next_toggle_at = now + self.toggle_seconds
        return self.value


class ControllerWatchdogStartupGuard:
    """Re-arm and prove the realtime userspace heartbeat before E-stop reset."""

    def __init__(self) -> None:
        self.phase = PHASE_WAIT_PREREQUISITES
        self.phase_deadline: float | None = None
        self.stable_since: float | None = None
        self.enable = False
        self.ready = False
        self.runtime_committed = False
        self.fault: str | None = None
        self.arm_attempts = 0

    @property
    def faulted(self) -> bool:
        return self.fault is not None

    def _fail(self, reason: str) -> None:
        if self.fault is not None:
            return
        self.phase = PHASE_FAULT
        self.enable = False
        self.ready = False
        self.fault = reason

    def commit_runtime(self) -> None:
        if self.faulted or not self.ready:
            raise RuntimeError("cannot commit an unhealthy startup watchdog")
        self.runtime_committed = True

    def _begin_low_phase(self, now: float) -> None:
        self.phase = PHASE_ARM_LOW
        self.enable = False
        self.ready = False
        self.stable_since = None
        self.phase_deadline = now + WATCHDOG_ARM_LOW_SECONDS

    def _begin_prerequisite_wait(self) -> None:
        self.phase = PHASE_WAIT_PREREQUISITES
        self.phase_deadline = None
        self.stable_since = None
        self.enable = False
        self.ready = False

    def update(
        self,
        *,
        now: float,
        watchdog_ok: bool,
        prerequisites_ready: bool,
    ) -> None:
        if self.faulted:
            return

        if self.ready:
            if not self.runtime_committed and not prerequisites_ready:
                self._begin_prerequisite_wait()
                return
            if not watchdog_ok:
                if self.runtime_committed:
                    self._fail("controller heartbeat watchdog timed out after startup")
                else:
                    self._begin_low_phase(now)
            return

        if not prerequisites_ready:
            if self.phase != PHASE_WAIT_PREREQUISITES:
                self._begin_prerequisite_wait()
            return

        if self.phase == PHASE_WAIT_PREREQUISITES:
            self._begin_low_phase(now)
            return

        if self.phase == PHASE_ARM_LOW:
            if self.phase_deadline is None or now < self.phase_deadline:
                return
            self.enable = True
            self.arm_attempts += 1
            self.phase = PHASE_WAIT_OK
            self.phase_deadline = now + WATCHDOG_OK_WAIT_SECONDS
            return

        if self.phase == PHASE_WAIT_OK:
            if watchdog_ok:
                self.phase = PHASE_STABLE
                self.stable_since = now
                self.phase_deadline = None
                return
            if self.phase_deadline is not None and now >= self.phase_deadline:
                self._begin_low_phase(now)
            return

        if self.phase == PHASE_STABLE:
            if not watchdog_ok:
                self._begin_low_phase(now)
                return
            if (
                self.stable_since is not None
                and now - self.stable_since >= WATCHDOG_STABLE_SECONDS
            ):
                self.phase = PHASE_READY
                self.phase_deadline = None
                self.ready = True
