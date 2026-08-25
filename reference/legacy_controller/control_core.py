from __future__ import annotations

from dataclasses import dataclass

from motion_config import scale_reference_value
from pendant_protocol import PendantPacket


MOTOR_BY_AXIS = {"X": 1, "Y": 0, "Z": 2}
LIMIT_INPUT_BY_MOTOR = {0: 9, 1: 11, 2: 10}
PULSES_BY_MULTIPLIER = {"X1": 10, "X10": 100, "X100": 1000}
BOUNCE_PULSES = scale_reference_value(50)
BOUNCE_RATE_PULSES_PER_SECOND = float(scale_reference_value(300))
PREVIOUS_JOG_RATE_BY_MULTIPLIER = {
    "X1": float(scale_reference_value(500)),
    "X10": float(scale_reference_value(3000)),
    "X100": float(scale_reference_value(3000) * 2),
}
JOG_RATE_SCALE_BY_MULTIPLIER = {"X1": 2, "X10": 5, "X100": 10}
JOG_RATE_BY_MULTIPLIER = {
    multiplier: PREVIOUS_JOG_RATE_BY_MULTIPLIER[multiplier] * scale
    for multiplier, scale in JOG_RATE_SCALE_BY_MULTIPLIER.items()
}
JOG_RATE_PULSES_PER_SECOND = max(JOG_RATE_BY_MULTIPLIER.values())

# User-confirmed physical handwheel mapping. The Nano decoder reports its
# measured clockwise quadrature sequence as a positive detent.
CLOCKWISE_SIGN_BY_AXIS = {"X": -1, "Y": 1, "Z": 1}


class ControlFault(RuntimeError):
    pass


@dataclass(frozen=True)
class JogRequest:
    motor: int
    delta_pulses: int
    axis: str
    multiplier: str
    detent_delta: int


@dataclass(frozen=True)
class PendantDecision:
    stop: bool
    reason: str
    jog: JogRequest | None = None


class PendantInterpreter:
    """Convert latest-wins P3 pendant samples into finite pulse requests."""

    def __init__(self):
        self.previous: PendantPacket | None = None
        self.armed_selection: tuple[str, str] | None = None

    def reset(self) -> None:
        """Discard all wheel and selector history after a machine-side reset."""
        self.previous = None
        self.armed_selection = None

    def _store_and_stop(self, packet: PendantPacket, reason: str) -> PendantDecision:
        self.previous = packet
        self.armed_selection = None
        return PendantDecision(stop=True, reason=reason)

    def process(self, packet: PendantPacket) -> PendantDecision:
        previous = self.previous
        if previous is None:
            return self._store_and_stop(packet, "initial packet establishes baseline")

        sequence_delta = (packet.sequence - previous.sequence) & 0xFFFFFFFF
        if sequence_delta == 0 or sequence_delta > 0x7FFFFFFF:
            return self._store_and_stop(packet, "packet sequence restarted or repeated")

        if packet.quadrature_errors != previous.quadrature_errors:
            self.previous = packet
            self.armed_selection = None
            raise ControlFault(
                "Nano quadrature error count changed; manual control stopped"
            )

        self.previous = packet

        if packet.estop_pressed:
            self.armed_selection = None
            return PendantDecision(stop=True, reason="pendant E-stop is pressed/open")
        if not packet.selector_valid:
            self.armed_selection = None
            return PendantDecision(stop=True, reason="selector state is invalid")
        if packet.axis not in MOTOR_BY_AXIS:
            self.armed_selection = None
            return PendantDecision(stop=True, reason="selected axis is not X, Y, or Z")
        if packet.multiplier not in PULSES_BY_MULTIPLIER:
            self.armed_selection = None
            return PendantDecision(stop=True, reason="multiplier is invalid")
        if not packet.deadman_held:
            self.armed_selection = None
            return PendantDecision(stop=True, reason="side-button deadman is released")

        selection = (packet.axis, packet.multiplier)
        if self.armed_selection != selection:
            self.armed_selection = selection
            return PendantDecision(
                stop=True,
                reason="deadman/selector activation establishes a fresh baseline",
            )

        detent_signal = packet.latest_detent_signal
        if detent_signal == 0:
            return PendantDecision(stop=False, reason="no wheel detent")

        machine_detents = detent_signal * CLOCKWISE_SIGN_BY_AXIS[packet.axis]
        delta_pulses = machine_detents * PULSES_BY_MULTIPLIER[packet.multiplier]
        return PendantDecision(
            stop=False,
            reason="wheel detent",
            jog=JogRequest(
                motor=MOTOR_BY_AXIS[packet.axis],
                delta_pulses=delta_pulses,
                axis=packet.axis,
                multiplier=packet.multiplier,
                detent_delta=detent_signal,
            ),
        )


RECOVERY_INACTIVE = "inactive"
RECOVERY_WAIT_ESTOP_RELEASE = "wait-estop-release"
RECOVERY_WAIT_X10 = "wait-axis-x-multiplier-x10"
RECOVERY_WAIT_X1 = "wait-axis-x-multiplier-x1"
RECOVERY_WAIT_OFF = "wait-axis-off"
RECOVERY_WAIT_CLOCKWISE = "wait-clockwise"
RECOVERY_WAIT_COUNTERCLOCKWISE = "wait-counterclockwise"
RECOVERY_WAIT_BUTTON_PRESS = "wait-button-press"
RECOVERY_WAIT_BUTTON_RELEASE = "wait-button-release"
RECOVERY_COMPLETE = "complete-pending"


@dataclass(frozen=True)
class RecoveryUpdate:
    stage: str
    message: str | None = None
    unlock_requested: bool = False


class EstopRecoverySequence:
    """Recognize the user-defined no-motion E-stop recovery gesture."""

    def __init__(self) -> None:
        self.stage = RECOVERY_INACTIVE
        self.clicks = 0
        self.quadrature_errors: int | None = None

    @property
    def active(self) -> bool:
        return self.stage != RECOVERY_INACTIVE

    def engage(self, packet: PendantPacket) -> RecoveryUpdate:
        self.stage = RECOVERY_WAIT_ESTOP_RELEASE
        self.clicks = 0
        self.quadrature_errors = packet.quadrature_errors
        return RecoveryUpdate(
            stage=self.stage,
            message="E-stop pressed/open; recovery locked until release",
        )

    def accept_unlock(self) -> None:
        self.stage = RECOVERY_INACTIVE
        self.clicks = 0
        self.quadrature_errors = None

    def restart(self, packet: PendantPacket, reason: str) -> RecoveryUpdate:
        self.stage = RECOVERY_WAIT_X10
        self.clicks = 0
        self.quadrature_errors = packet.quadrature_errors
        return RecoveryUpdate(
            stage=self.stage,
            message=(
                f"recovery sequence restarted: {reason}; "
                "waiting for axis X + multiplier x10"
            ),
        )

    @staticmethod
    def _released_x10(packet: PendantPacket) -> bool:
        return (
            packet.axis == "X"
            and packet.multiplier == "X10"
            and packet.selector_valid
            and not packet.deadman_held
        )

    @staticmethod
    def _released_x1(packet: PendantPacket) -> bool:
        return (
            packet.axis == "X"
            and packet.multiplier == "X1"
            and packet.selector_valid
            and not packet.deadman_held
        )

    @staticmethod
    def _released_off(packet: PendantPacket) -> bool:
        return (
            packet.axis == "N"
            and packet.multiplier == "N"
            and not packet.selector_valid
            and not packet.deadman_held
        )

    @staticmethod
    def _off_x1_button_held(packet: PendantPacket) -> bool:
        return (
            packet.axis == "N"
            and packet.multiplier == "X1"
            and not packet.selector_valid
            and packet.deadman_held
        )

    def process(self, packet: PendantPacket) -> RecoveryUpdate:
        if not self.active:
            return RecoveryUpdate(stage=RECOVERY_INACTIVE)

        if packet.estop_pressed:
            changed = self.stage != RECOVERY_WAIT_ESTOP_RELEASE
            self.stage = RECOVERY_WAIT_ESTOP_RELEASE
            self.clicks = 0
            self.quadrature_errors = packet.quadrature_errors
            return RecoveryUpdate(
                stage=self.stage,
                message=(
                    "E-stop pressed again; recovery sequence reset"
                    if changed
                    else None
                ),
            )

        if self.quadrature_errors is None:
            self.quadrature_errors = packet.quadrature_errors
        elif packet.quadrature_errors != self.quadrature_errors:
            return self.restart(packet, "quadrature error count changed")

        signal = packet.latest_detent_signal

        if self.stage == RECOVERY_WAIT_ESTOP_RELEASE:
            self.clicks = 0
            if self._released_x10(packet) and signal == 0:
                self.stage = RECOVERY_WAIT_X1
                return RecoveryUpdate(
                    stage=self.stage,
                    message=(
                        "E-stop released and X+x10 confirmed; "
                        "waiting for X+x1"
                    ),
                )
            self.stage = RECOVERY_WAIT_X10
            return RecoveryUpdate(
                stage=self.stage,
                message="E-stop released; waiting for axis X + multiplier x10",
            )

        if self.stage == RECOVERY_WAIT_X10:
            if self._released_x10(packet) and signal == 0:
                self.stage = RECOVERY_WAIT_X1
                return RecoveryUpdate(
                    stage=self.stage,
                    message="X+x10 confirmed; waiting for multiplier x1",
                )
            return RecoveryUpdate(stage=self.stage)

        if self.stage == RECOVERY_WAIT_X1:
            if self._released_x10(packet) and signal == 0:
                return RecoveryUpdate(stage=self.stage)
            if self._released_x1(packet) and signal == 0:
                self.stage = RECOVERY_WAIT_OFF
                return RecoveryUpdate(
                    stage=self.stage,
                    message="X+x10 to X+x1 transition confirmed; waiting for OFF",
                )
            return self.restart(packet, "expected multiplier x1 while axis stayed X")

        if self.stage == RECOVERY_WAIT_OFF:
            if self._released_x1(packet) and signal == 0:
                return RecoveryUpdate(stage=self.stage)
            if self._released_off(packet) and signal == 0:
                self.stage = RECOVERY_WAIT_CLOCKWISE
                return RecoveryUpdate(
                    stage=self.stage,
                    message="axis OFF confirmed; waiting for clockwise wheel motion",
                )
            return self.restart(packet, "expected axis OFF directly after X+x1")

        if self.stage == RECOVERY_WAIT_CLOCKWISE:
            if not self._released_off(packet) or packet.deadman_held:
                return self.restart(packet, "selector/button changed before clockwise")
            if signal == 0:
                return RecoveryUpdate(stage=self.stage)
            if signal == +1:
                self.stage = RECOVERY_WAIT_COUNTERCLOCKWISE
                return RecoveryUpdate(
                    stage=self.stage,
                    message="clockwise confirmed; waiting for counterclockwise",
                )
            return self.restart(packet, "counterclockwise occurred before clockwise")

        if self.stage == RECOVERY_WAIT_COUNTERCLOCKWISE:
            if not self._released_off(packet) or packet.deadman_held:
                return self.restart(
                    packet, "selector/button changed before counterclockwise"
                )
            if signal in (0, +1):
                return RecoveryUpdate(stage=self.stage)
            self.stage = RECOVERY_WAIT_BUTTON_PRESS
            return RecoveryUpdate(
                stage=self.stage,
                message=(
                    "counterclockwise confirmed; waiting for side-button click 1/3 "
                    "with x1"
                ),
            )

        if self.stage == RECOVERY_WAIT_BUTTON_PRESS:
            if self._released_off(packet):
                if signal in (0, -1) and self.clicks == 0:
                    return RecoveryUpdate(stage=self.stage)
                if signal == 0:
                    return RecoveryUpdate(stage=self.stage)
                return self.restart(packet, "wheel moved after button phase began")
            if self._off_x1_button_held(packet) and signal == 0:
                self.stage = RECOVERY_WAIT_BUTTON_RELEASE
                return RecoveryUpdate(
                    stage=self.stage,
                    message=f"side-button press {self.clicks + 1}/3 confirmed",
                )
            return self.restart(packet, "button press did not confirm OFF with x1")

        if self.stage == RECOVERY_WAIT_BUTTON_RELEASE:
            if self._off_x1_button_held(packet) and signal == 0:
                return RecoveryUpdate(stage=self.stage)
            if self._released_off(packet) and signal == 0:
                self.clicks += 1
                if self.clicks == 3:
                    self.stage = RECOVERY_COMPLETE
                    return RecoveryUpdate(
                        stage=self.stage,
                        message="three complete side-button clicks confirmed",
                        unlock_requested=True,
                    )
                self.stage = RECOVERY_WAIT_BUTTON_PRESS
                return RecoveryUpdate(
                    stage=self.stage,
                    message=(
                        f"side-button click {self.clicks}/3 complete; "
                        f"waiting for click {self.clicks + 1}/3"
                    ),
                )
            return self.restart(packet, "button was not released at OFF with x1")

        if self.stage == RECOVERY_COMPLETE:
            return RecoveryUpdate(stage=self.stage, unlock_requested=True)

        return self.restart(packet, "unknown recovery state")


@dataclass(frozen=True)
class LimitAction:
    kind: str
    motor: int | None = None
    bounce_delta: int = 0


@dataclass(frozen=True)
class BouncePlan:
    motor: int
    stopped_count: int
    target_count: int

    @property
    def delta_pulses(self) -> int:
        return self.target_count - self.stopped_count


def make_bounce_plan(*, motor: int, stopped_count: int) -> BouncePlan:
    if motor not in LIMIT_INPUT_BY_MOTOR:
        raise ValueError(f"motor {motor} has no mapped limit input")
    return BouncePlan(
        motor=motor,
        stopped_count=stopped_count,
        target_count=stopped_count - BOUNCE_PULSES,
    )


def decide_limit_action(
    *,
    active_motor: int | None,
    active_toward_limit: bool,
    latched_by_motor: tuple[bool, bool, bool],
) -> LimitAction:
    active_limits = [index for index, active in enumerate(latched_by_motor) if active]
    if not active_limits:
        return LimitAction("none")
    if (
        len(active_limits) == 1
        and active_motor is not None
        and active_limits[0] == active_motor
        and active_toward_limit
    ):
        return LimitAction(
            "bounce", motor=active_motor, bounce_delta=-BOUNCE_PULSES
        )
    return LimitAction("fault")
