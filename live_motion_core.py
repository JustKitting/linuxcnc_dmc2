from __future__ import annotations

import math
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Protocol

PROJECT_ROOT = Path(__file__).resolve().parents[1]
for _import_directory in (PROJECT_ROOT / "pendant_nano", PROJECT_ROOT / "pendant_cnc"):
    if str(_import_directory) not in sys.path:
        sys.path.insert(0, str(_import_directory))

from pendant_protocol import PendantPacket

from control_core import (
    BOUNCE_PULSES,
    BOUNCE_RATE_PULSES_PER_SECOND,
    JOG_RATE_BY_MULTIPLIER,
    ControlFault,
    EstopRecoverySequence,
    JogRequest,
    PendantInterpreter,
)


AXIS_INDEX_BY_NAME = {"X": 0, "Y": 1, "Z": 2}
MOTOR_BY_AXIS_INDEX = {0: 1, 1: 0, 2: 2}
AXIS_INDEX_BY_MOTOR = {motor: axis for axis, motor in MOTOR_BY_AXIS_INDEX.items()}

PHASE_IDLE = "idle"
PHASE_STOPPING_CANCEL = "stopping-cancel"
PHASE_STOPPING_REPLACE = "stopping-replace"
PHASE_STOPPING_BOUNCE = "stopping-bounce"
PHASE_BOUNCING = "bouncing"
PHASE_BOUNCE_RESET = "bounce-reset"
PHASE_STARTUP_GATE_SETTLE = "startup-gate-settle"
PHASE_STARTUP_WAIT_RESET = "startup-wait-reset"
PHASE_STARTUP_WAIT_ON = "startup-wait-on"
PHASE_STARTUP_READY_GATE_SETTLE = "startup-ready-gate-settle"
PHASE_STARTUP_READY_WAIT_RESET = "startup-ready-wait-reset"
PHASE_STARTUP_READY_WAIT_ON = "startup-ready-wait-on"

STARTUP_BOUNCE_PREP_PHASES = {
    PHASE_STARTUP_GATE_SETTLE,
    PHASE_STARTUP_WAIT_RESET,
    PHASE_STARTUP_WAIT_ON,
}

STARTUP_POWER_PHASES = {
    PHASE_STARTUP_READY_GATE_SETTLE,
    PHASE_STARTUP_READY_WAIT_RESET,
    PHASE_STARTUP_READY_WAIT_ON,
}

RECOVERY_GATE_SETTLE = "gate-settle"
RECOVERY_WAIT_RESET = "wait-reset"
RECOVERY_WAIT_ON = "wait-on"


@dataclass(frozen=True)
class LinkSnapshot:
    connected: bool
    serial_fault: bool
    quadrature_fault: bool
    estop_pressed: bool


@dataclass(frozen=True)
class MachineSnapshot:
    machine_on: bool
    estopped: bool
    manual_mode: bool
    joint_mode: bool
    teleop_mode: bool
    interp_idle: bool
    homed: tuple[bool, bool, bool]
    homing: tuple[bool, bool, bool]
    axis_stopped: tuple[bool, bool, bool]

    @property
    def all_homed(self) -> bool:
        return all(self.homed)

    @property
    def any_homing(self) -> bool:
        return any(self.homing)

    @property
    def ready_for_pendant_jog(self) -> bool:
        jog_mode_ready = self.teleop_mode if self.all_homed else self.joint_mode
        return (
            self.machine_on
            and not self.estopped
            and self.manual_mode
            and jog_mode_ready
            and self.interp_idle
            and not self.any_homing
        )


class CommandBackend(Protocol):
    def stop_axis(self, axis_index: int, *, joint_jog: bool) -> None: ...

    def abort(self) -> None: ...

    def prepare_manual_teleop(self) -> None: ...

    def jog_increment(
        self,
        axis_index: int,
        signed_velocity: float,
        distance: float,
        *,
        joint_jog: bool,
    ) -> None: ...

    def request_estop_reset(self) -> None: ...

    def request_machine_on(self) -> None: ...

    def clear_state_requests(self) -> None: ...


@dataclass
class ActiveJog:
    request: JogRequest
    axis_index: int
    joint_jog: bool
    start_count: int
    target_count: int
    issued_at: float

    @property
    def motor(self) -> int:
        return self.request.motor

    @property
    def toward_positive_limit(self) -> bool:
        return self.request.delta_pulses > 0


class LinuxCncPendantSupervisor:
    """Pure supervisory state machine for the LinuxCNC-owned Mesa setup.

    All physical motion is requested through ``CommandBackend``.  The class
    never owns or loads a Mesa driver.  A pending handwheel request is a
    one-element replaceable slot; it is never accumulated into a queue.
    """

    def __init__(
        self,
        backend: CommandBackend,
        *,
        pulses_per_mm: int,
        packet_timeout_seconds: float = 0.100,
    ) -> None:
        if pulses_per_mm <= 0:
            raise ValueError("pulses_per_mm must be positive")
        if packet_timeout_seconds <= 0:
            raise ValueError("packet timeout must be positive")

        self.backend = backend
        self.pulses_per_mm = pulses_per_mm
        self.packet_timeout_seconds = packet_timeout_seconds
        self.interpreter = PendantInterpreter()
        self.recovery = EstopRecoverySequence()

        self.last_packet_at: float | None = None
        self.last_packet: PendantPacket | None = None
        self.link_established = False
        self.startup_limits_checked = False
        self.startup_reset_complete = False

        self.phase = PHASE_IDLE
        self.active: ActiveJog | None = None
        self.pending: JogRequest | None = None
        self.collision_motor: int | None = None
        self.bounce_start_count: int | None = None
        self.phase_deadline: float | None = None
        self.motion_not_before = 0.0

        self.external_enable = False
        self.control_available = False
        self.control_ready = False
        self.pendant_mode_enabled = False
        self.fault: str | None = None
        self.recovery_phase: str | None = None

        self.limit_reset = [False, False, False]
        self.limit_reset_until: float | None = None
        self.homing_was_active = False
        self.last_message: str | None = None

    @property
    def faulted(self) -> bool:
        return self.fault is not None

    @property
    def bounce_active(self) -> bool:
        return self.phase in {
            PHASE_STOPPING_BOUNCE,
            PHASE_BOUNCING,
            PHASE_BOUNCE_RESET,
            *STARTUP_BOUNCE_PREP_PHASES,
        }

    @property
    def recovery_active(self) -> bool:
        return self.recovery.active or self.recovery_phase is not None

    @property
    def command_enabled_by_motor(self) -> tuple[bool, bool, bool]:
        """Expose the old controller's exact realtime command-enable state."""
        enabled = [False, False, False]
        if self.active is None:
            return tuple(enabled)
        if self.phase in {
            PHASE_IDLE,
            PHASE_BOUNCING,
            *STARTUP_BOUNCE_PREP_PHASES,
        }:
            enabled[self.active.motor] = True
        return tuple(enabled)

    @property
    def toward_limit_by_motor(self) -> tuple[bool, bool, bool]:
        """Expose the old controller's exact per-motor direction gate state."""
        toward = [False, False, False]
        if self.active is not None and self.phase == PHASE_IDLE:
            toward[self.active.motor] = self.active.toward_positive_limit
        return tuple(toward)

    def fail(self, reason: str) -> None:
        """Enter the same fail-closed state for an adapter/runtime failure."""
        self._fault(reason)

    def restart_uncommitted_startup(self, *, now: float) -> None:
        """Retry a no-motion startup gate sequence after watchdog re-arming."""
        if self.startup_reset_complete:
            return
        self.backend.clear_state_requests()
        self.external_enable = False
        self.phase_deadline = None
        if self.phase in STARTUP_POWER_PHASES:
            self.phase = PHASE_STARTUP_READY_GATE_SETTLE
            self.motion_not_before = now + 0.050
            self._message(
                "startup watchdog re-armed; retrying automatic LinuxCNC reset"
            )
            return
        if self.phase in STARTUP_BOUNCE_PREP_PHASES:
            self.phase = PHASE_STARTUP_GATE_SETTLE
            self.motion_not_before = now + 0.050
            self._message(
                "startup watchdog re-armed; retrying startup-bounce reset"
            )
            return
        if self.recovery_phase in {
            RECOVERY_GATE_SETTLE,
            RECOVERY_WAIT_RESET,
            RECOVERY_WAIT_ON,
        }:
            self.recovery_phase = RECOVERY_GATE_SETTLE
            self.motion_not_before = now + 0.050
            self._message(
                "startup watchdog re-armed; retrying E-stop recovery reset"
            )

    def _message(self, message: str) -> None:
        if message != self.last_message:
            print(f"DMC2 PENDANT: {message}", flush=True)
            self.last_message = message

    def _stop_active_axis(self) -> None:
        if self.active is not None:
            self.backend.stop_axis(
                self.active.axis_index,
                joint_jog=self.active.joint_jog,
            )

    def _fault(self, reason: str) -> None:
        if self.fault is not None:
            return
        self._stop_active_axis()
        self.backend.abort()
        self.active = None
        self.pending = None
        self.phase = PHASE_IDLE
        self.collision_motor = None
        self.bounce_start_count = None
        self.phase_deadline = None
        self.limit_reset = [False, False, False]
        self.limit_reset_until = None
        self.external_enable = False
        self.control_available = False
        self.control_ready = False
        self.backend.clear_state_requests()
        self.fault = reason
        self._message(f"FAULT — {reason}; LinuxCNC restart required")

    def _cancel_pendant_motion(self, reason: str) -> None:
        self.pending = None
        if self.active is None:
            return
        if self.bounce_active:
            return
        self.backend.stop_axis(
            self.active.axis_index,
            joint_jog=self.active.joint_jog,
        )
        self.phase = PHASE_STOPPING_CANCEL
        self._message(f"jog stopping — {reason}")

    def _set_pendant_mode(self, enabled: bool) -> None:
        """Apply one fail-closed mode edge without suppressing safety logic."""
        enabled = bool(enabled)
        if enabled == self.pendant_mode_enabled:
            return
        self.pendant_mode_enabled = enabled
        self.interpreter.reset()
        self.pending = None
        if enabled:
            self._message("Pendant Mode armed; fresh input baseline required")
            return
        self._cancel_pendant_motion("Pendant Mode disabled")
        self._message("Pendant Mode disarmed; pendant jog requests ignored")

    def _engage_estop(self, packet: PendantPacket) -> None:
        self._stop_active_axis()
        self.backend.abort()
        self.active = None
        self.pending = None
        self.phase = PHASE_IDLE
        self.collision_motor = None
        self.bounce_start_count = None
        self.phase_deadline = None
        self.limit_reset = [False, False, False]
        self.limit_reset_until = None
        self.external_enable = False
        self.control_available = False
        self.control_ready = False
        self.backend.clear_state_requests()
        self.recovery_phase = None
        self.interpreter.reset()
        update = self.recovery.engage(packet)
        if update.message:
            self._message(update.message)

    def _begin_recovery_unlock(
        self,
        *,
        now: float,
        raw_limits: tuple[bool, bool, bool],
        safety_limits: tuple[bool, bool, bool],
        machine: MachineSnapshot,
        packet: PendantPacket,
    ) -> None:
        if any(raw_limits) or any(safety_limits):
            update = self.recovery.restart(
                packet, "a raw or realtime-latched limit is active"
            )
            if update.message:
                self._message(update.message)
            return
        if machine.any_homing:
            update = self.recovery.restart(packet, "homing is active")
            if update.message:
                self._message(update.message)
            return

        # Make the external E-stop chain healthy first.  The realtime latch
        # must observe this before the NML reset request is issued.
        self.backend.clear_state_requests()
        self.external_enable = True
        self.recovery_phase = RECOVERY_GATE_SETTLE
        self.motion_not_before = now + 0.050
        self._message("recovery gesture complete; settling external E-stop gate")

    def _advance_recovery(
        self,
        *,
        now: float,
        machine: MachineSnapshot,
        link: LinkSnapshot,
        raw_limits: tuple[bool, bool, bool],
        safety_limits: tuple[bool, bool, bool],
    ) -> None:
        if self.recovery_phase is None:
            return
        if link.estop_pressed:
            if self.last_packet is not None:
                self._engage_estop(self.last_packet)
            return
        if any(raw_limits) or any(safety_limits):
            self._fault("a limit became active during E-stop recovery")
            return
        if self.recovery_phase == RECOVERY_GATE_SETTLE:
            if now < self.motion_not_before:
                return
            self.backend.request_estop_reset()
            self.recovery_phase = RECOVERY_WAIT_RESET
            self._message("external gate healthy; waiting for LinuxCNC E-stop reset")
            return

        if self.recovery_phase == RECOVERY_WAIT_RESET:
            if machine.estopped:
                return
            self.backend.request_machine_on()
            self.recovery_phase = RECOVERY_WAIT_ON
            self._message("LinuxCNC E-stop reset; waiting for machine-on")
            return

        if self.recovery_phase == RECOVERY_WAIT_ON:
            if not machine.machine_on:
                return
            self.backend.clear_state_requests()
            self.recovery.accept_unlock()
            self.recovery_phase = None
            self.interpreter.reset()
            self._message(
                "E-stop recovery complete; fresh selector/deadman baseline required"
            )

    def _start_jog(
        self,
        request: JogRequest,
        *,
        now: float,
        machine: MachineSnapshot,
        counts_by_motor: tuple[int, int, int],
        raw_limits: tuple[bool, bool, bool],
        safety_limits: tuple[bool, bool, bool],
    ) -> None:
        if not machine.ready_for_pendant_jog:
            self._message(
                "jog ignored — machine must be on, idle, manual, and in the "
                "correct joint/axis jog mode"
            )
            return
        if any(raw_limits) or any(safety_limits):
            self._message("jog ignored — a raw or realtime-latched limit is active")
            return

        axis_index = AXIS_INDEX_BY_NAME[request.axis]
        start_count = counts_by_motor[request.motor]
        velocity = JOG_RATE_BY_MULTIPLIER[request.multiplier] / self.pulses_per_mm
        if request.delta_pulses < 0:
            velocity = -velocity
        distance = abs(request.delta_pulses) / self.pulses_per_mm
        joint_jog = not machine.all_homed

        self.backend.jog_increment(
            axis_index,
            velocity,
            distance,
            joint_jog=joint_jog,
        )
        self.active = ActiveJog(
            request=request,
            axis_index=axis_index,
            joint_jog=joint_jog,
            start_count=start_count,
            target_count=start_count + request.delta_pulses,
            issued_at=now,
        )
        self.pending = None
        self.phase = PHASE_IDLE
        self.motion_not_before = now + 0.025
        self._message(
            f"jog {request.axis} {request.delta_pulses:+d} pulses "
            f"at {abs(velocity):g} mm/s"
        )

    def _request_jog(
        self,
        request: JogRequest,
        *,
        now: float,
        machine: MachineSnapshot,
        counts_by_motor: tuple[int, int, int],
        raw_limits: tuple[bool, bool, bool],
        safety_limits: tuple[bool, bool, bool],
    ) -> None:
        if self.bounce_active:
            return
        if self.active is None:
            self._start_jog(
                request,
                now=now,
                machine=machine,
                counts_by_motor=counts_by_motor,
                raw_limits=raw_limits,
                safety_limits=safety_limits,
            )
            return

        active = self.active

        # Match the previously validated direct-stepgen controller: while the
        # same motor is already moving in the same direction, replace its
        # outstanding target with exactly one sampled increment from the
        # current generated count.  LinuxCNC's JOG_INCREMENT adds to its
        # existing planner target, so send only the extension between the old
        # target and that replacement target.  This keeps motion continuous
        # without accumulating every handwheel event into a queue.
        same_motion = (
            self.phase == PHASE_IDLE
            and active.motor == request.motor
            and active.axis_index == AXIS_INDEX_BY_NAME[request.axis]
            and active.joint_jog == (not machine.all_homed)
            and active.toward_positive_limit == (request.delta_pulses > 0)
        )
        if same_motion:
            current_count = counts_by_motor[request.motor]
            replacement_target = current_count + request.delta_pulses
            extension_pulses = replacement_target - active.target_count
            extension_is_forward = extension_pulses > 0
            request_is_forward = request.delta_pulses > 0

            if extension_pulses == 0:
                active.request = request
                self.pending = None
                self._message(
                    f"jog target retained: {request.axis} "
                    f"{request.delta_pulses:+d} pulses; target "
                    f"{replacement_target}"
                )
                return

            if extension_is_forward == request_is_forward:
                velocity = (
                    JOG_RATE_BY_MULTIPLIER[request.multiplier]
                    / self.pulses_per_mm
                )
                if request.delta_pulses < 0:
                    velocity = -velocity
                self.backend.jog_increment(
                    active.axis_index,
                    velocity,
                    abs(extension_pulses) / self.pulses_per_mm,
                    joint_jog=active.joint_jog,
                )
                active.request = request
                active.target_count = replacement_target
                active.issued_at = now
                self.pending = None
                self.motion_not_before = now + 0.025
                self._message(
                    f"jog target replaced: {request.axis} target "
                    f"{replacement_target}, extension {extension_pulses:+d} pulses"
                )
                return

        # One replaceable slot only.  A second or millionth detent observed
        # while a stop is required overwrites this same object.
        self.pending = request
        if self.phase != PHASE_STOPPING_REPLACE:
            self.backend.stop_axis(
                self.active.axis_index,
                joint_jog=self.active.joint_jog,
            )
            self.phase = PHASE_STOPPING_REPLACE
        self._message(
            f"latest-wins replacement stored: {request.axis} "
            f"{request.delta_pulses:+d} pulses"
        )

    def _begin_bounce(self, *, motor: int, now: float) -> None:
        if self.active is None:
            self._fault("limit collision had no attributable pendant jog")
            return
        self.backend.stop_axis(
            self.active.axis_index,
            joint_jog=self.active.joint_jog,
        )
        self.backend.abort()
        self.pending = None
        self.phase = PHASE_STOPPING_BOUNCE
        self.collision_motor = motor
        self.bounce_start_count = None
        self.phase_deadline = now + 2.0
        self._message(
            f"limit collision on motor {motor}; active jog stopped; preparing exact "
            f"-{BOUNCE_PULSES}-pulse bounce"
        )

    def _begin_startup_power(self, *, now: float) -> None:
        self.backend.clear_state_requests()
        self.phase = PHASE_STARTUP_READY_GATE_SETTLE
        self.motion_not_before = now + 0.050
        self.phase_deadline = None
        self.external_enable = True
        self._message(
            "startup inputs valid; settling external gate before automatic "
            "LinuxCNC reset"
        )

    def _finish_startup_power(self) -> None:
        self.backend.clear_state_requests()
        self.phase = PHASE_IDLE
        self.phase_deadline = None
        self.startup_reset_complete = True
        self.interpreter.reset()
        self._message(
            "startup reset complete; fresh selector/deadman baseline required"
        )

    def _advance_startup_power(
        self,
        *,
        now: float,
        machine: MachineSnapshot,
        raw_limits: tuple[bool, bool, bool],
        safety_limits: tuple[bool, bool, bool],
    ) -> None:
        if self.phase not in STARTUP_POWER_PHASES:
            return
        if any(raw_limits) or any(safety_limits):
            self._fault("a limit became active during startup reset")
            return
        if machine.any_homing:
            self._fault("homing became active during startup reset")
            return
        if self.phase == PHASE_STARTUP_READY_GATE_SETTLE:
            if now < self.motion_not_before:
                return
            if machine.estopped:
                self.backend.request_estop_reset()
                self.phase = PHASE_STARTUP_READY_WAIT_RESET
                self._message(
                    "startup gate healthy; waiting for LinuxCNC E-stop reset"
                )
                return
            if not machine.machine_on:
                self.backend.request_machine_on()
                self.phase = PHASE_STARTUP_READY_WAIT_ON
                self._message("LinuxCNC E-stop clear; waiting for machine-on")
                return
            self._finish_startup_power()
            return

        if self.phase == PHASE_STARTUP_READY_WAIT_RESET:
            if machine.estopped:
                return
            if not machine.machine_on:
                self.backend.request_machine_on()
                self.phase = PHASE_STARTUP_READY_WAIT_ON
                self._message("LinuxCNC E-stop reset; waiting for machine-on")
                return
            self._finish_startup_power()
            return

        if self.phase == PHASE_STARTUP_READY_WAIT_ON:
            if not machine.machine_on:
                return
            self._finish_startup_power()

    def _begin_startup_bounce(
        self,
        *,
        motor: int,
        now: float,
        machine: MachineSnapshot,
        counts_by_motor: tuple[int, int, int],
    ) -> None:
        self.backend.clear_state_requests()
        axis_index = AXIS_INDEX_BY_MOTOR[motor]
        axis_name = next(
            name for name, index in AXIS_INDEX_BY_NAME.items() if index == axis_index
        )
        start_count = counts_by_motor[motor]
        request = JogRequest(
            motor=motor,
            delta_pulses=-BOUNCE_PULSES,
            axis=axis_name,
            multiplier="X1",
            detent_delta=0,
        )
        self.active = ActiveJog(
            request=request,
            axis_index=axis_index,
            joint_jog=not machine.all_homed,
            start_count=start_count,
            target_count=start_count - BOUNCE_PULSES,
            issued_at=now,
        )
        self.pending = None
        self.collision_motor = motor
        self.bounce_start_count = None
        self.phase = PHASE_STARTUP_GATE_SETTLE
        self.motion_not_before = now + 0.050
        self.phase_deadline = None
        self.external_enable = True
        self._message(
            f"startup found motor {motor} at its limit; arming exact "
            f"-{BOUNCE_PULSES}-pulse bounce at "
            f"{BOUNCE_RATE_PULSES_PER_SECOND:g} pulses/s"
        )

    def _check_startup_limits(
        self,
        *,
        now: float,
        machine: MachineSnapshot,
        counts_by_motor: tuple[int, int, int],
        raw_limits: tuple[bool, bool, bool],
        safety_limits: tuple[bool, bool, bool],
    ) -> None:
        if self.startup_limits_checked:
            return
        self.startup_limits_checked = True

        if not any(raw_limits) and not any(safety_limits):
            if machine.estopped or not machine.machine_on:
                self._begin_startup_power(now=now)
            else:
                self.backend.clear_state_requests()
                self.startup_reset_complete = True
            return
        active_raw = [motor for motor, active in enumerate(raw_limits) if active]
        active_safety = [
            motor for motor, active in enumerate(safety_limits) if active
        ]
        if len(active_raw) != 1 or (
            active_safety and active_raw != active_safety
        ):
            self._fault(
                "startup limits were not one matching input: "
                f"raw={raw_limits}, latched={safety_limits}"
            )
            return
        if machine.any_homing:
            self._fault("startup limit bounce refused because homing is active")
            return
        self._begin_startup_bounce(
            motor=active_raw[0],
            now=now,
            machine=machine,
            counts_by_motor=counts_by_motor,
        )

    def _advance_startup_bounce(
        self,
        *,
        now: float,
        machine: MachineSnapshot,
        counts_by_motor: tuple[int, int, int],
        position_feedback_by_motor: tuple[float, float, float] | None,
        safety_limits: tuple[bool, bool, bool],
    ) -> None:
        if self.phase not in STARTUP_BOUNCE_PREP_PHASES:
            return
        motor = self.collision_motor
        expected = tuple(index == motor for index in range(3))
        if motor is None or (any(safety_limits) and safety_limits != expected):
            self._fault(
                "startup bounce lost its single matching safety latch: "
                f"{safety_limits}"
            )
            return
        if safety_limits != expected:
            return

        if self.phase == PHASE_STARTUP_GATE_SETTLE:
            if now < self.motion_not_before:
                return
            if machine.estopped:
                self.backend.request_estop_reset()
                self.phase = PHASE_STARTUP_WAIT_RESET
                self._message(
                    "startup bounce gate armed; waiting for LinuxCNC E-stop reset"
                )
                return
            if not machine.machine_on:
                self.backend.request_machine_on()
                self.phase = PHASE_STARTUP_WAIT_ON
                self._message(
                    "startup bounce gate armed; waiting for LinuxCNC machine-on"
                )
                return

        elif self.phase == PHASE_STARTUP_WAIT_RESET:
            if machine.estopped:
                return
            if not machine.machine_on:
                self.backend.request_machine_on()
                self.phase = PHASE_STARTUP_WAIT_ON
                self._message(
                    "LinuxCNC E-stop reset; waiting for startup-bounce machine-on"
                )
                return

        elif self.phase == PHASE_STARTUP_WAIT_ON:
            if not machine.machine_on:
                return

        if not machine.ready_for_pendant_jog:
            return
        self.backend.clear_state_requests()
        self.startup_reset_complete = True
        self._start_bounce_move(
            now=now,
            counts_by_motor=counts_by_motor,
            position_feedback_by_motor=position_feedback_by_motor,
            safety_limits=safety_limits,
        )

    def _check_limits(
        self,
        *,
        now: float,
        machine: MachineSnapshot,
        safety_limits: tuple[bool, bool, bool],
    ) -> None:
        if not any(safety_limits):
            return
        if machine.any_homing or self.homing_was_active:
            return

        active_motors = [motor for motor, active in enumerate(safety_limits) if active]
        if self.bounce_active:
            if self.collision_motor is None or active_motors != [self.collision_motor]:
                self._fault(
                    "unexpected or multiple realtime-latched limits during bounce: "
                    f"{safety_limits}"
                )
            return

        if (
            self.active is not None
            and self.active.toward_positive_limit
            and active_motors == [self.active.motor]
        ):
            self._begin_bounce(motor=self.active.motor, now=now)
            return

        self._fault(
            "limit event was not one matching positive pendant collision: "
            f"{safety_limits}"
        )

    def _start_bounce_move(
        self,
        *,
        now: float,
        counts_by_motor: tuple[int, int, int],
        position_feedback_by_motor: tuple[float, float, float] | None,
        safety_limits: tuple[bool, bool, bool],
    ) -> None:
        motor = self.collision_motor
        if motor is None or list(safety_limits) != [motor == 0, motor == 1, motor == 2]:
            self._fault("bounce start lost its single matching safety latch")
            return
        joint_jog = self.active.joint_jog
        start_count = counts_by_motor[motor]
        if position_feedback_by_motor is None:
            self._fault(
                f"motor {motor} fractional position feedback was unavailable "
                "at bounce start"
            )
            return
        start_position_pulses = (
            position_feedback_by_motor[motor] * self.pulses_per_mm
        )
        fractional_phase = start_position_pulses - start_count
        phase_tolerance = 0.000_001
        if (
            not math.isfinite(fractional_phase)
            or fractional_phase < -phase_tolerance
            or fractional_phase > 1.0 + phase_tolerance
        ):
            self._fault(
                f"motor {motor} count/position feedback was incoherent at "
                f"bounce start: count={start_count}, position="
                f"{start_position_pulses:.9f} pulses"
            )
            return
        fractional_phase = min(
            max(fractional_phase, 0.0),
            math.nextafter(1.0, 0.0),
        )

        # HostMot2 emits a negative step each time its 16.16 accumulator
        # crosses an integer boundary.  Put the final accumulator phase in
        # the middle of the target count bucket so the physical count delta
        # is exactly -BOUNCE_PULSES from every starting fractional phase.
        distance_pulses = BOUNCE_PULSES - 0.5 + fractional_phase
        if not joint_jog:
            self.backend.prepare_manual_teleop()
        axis_index = AXIS_INDEX_BY_MOTOR[motor]
        velocity = -(BOUNCE_RATE_PULSES_PER_SECOND / self.pulses_per_mm)
        distance = distance_pulses / self.pulses_per_mm
        self.backend.jog_increment(
            axis_index,
            velocity,
            distance,
            joint_jog=joint_jog,
        )
        self.bounce_start_count = start_count
        self.phase = PHASE_BOUNCING
        self.motion_not_before = now + 0.025
        self.phase_deadline = now + 2.0
        self._message(
            f"bounce motor {motor}: -{BOUNCE_PULSES} pulses at "
            f"{BOUNCE_RATE_PULSES_PER_SECOND:g} pulses/s; accumulator travel "
            f"{distance_pulses:.6f} pulse periods"
        )

    def _finish_bounce(
        self,
        *,
        now: float,
        counts_by_motor: tuple[int, int, int],
        raw_limits: tuple[bool, bool, bool],
    ) -> None:
        motor = self.collision_motor
        if motor is None or self.bounce_start_count is None:
            self._fault("bounce completion state was incomplete")
            return
        actual_delta = counts_by_motor[motor] - self.bounce_start_count
        if actual_delta != -BOUNCE_PULSES:
            self._fault(
                f"bounce count mismatch on motor {motor}: required "
                f"-{BOUNCE_PULSES}, generated {actual_delta:+d}"
            )
            return
        if raw_limits[motor]:
            self._fault(
                f"motor {motor} completed -{BOUNCE_PULSES} pulses but its raw "
                "limit remains active"
            )
            return

        self.limit_reset[motor] = True
        self.limit_reset_until = now + 0.010
        self.phase = PHASE_BOUNCE_RESET
        self.phase_deadline = now + 0.100
        self._message(
            f"bounce generated exactly {actual_delta:+d} pulses; resetting safety latch"
        )

    def _advance_motion(
        self,
        *,
        now: float,
        machine: MachineSnapshot,
        counts_by_motor: tuple[int, int, int],
        position_feedback_by_motor: tuple[float, float, float] | None,
        raw_limits: tuple[bool, bool, bool],
        safety_limits: tuple[bool, bool, bool],
    ) -> None:
        if self.phase_deadline is not None and now > self.phase_deadline:
            self._fault(f"motion timed out in phase {self.phase}")
            return

        if self.phase == PHASE_BOUNCE_RESET:
            if self.limit_reset_until is not None:
                if now < self.limit_reset_until:
                    return
                self.limit_reset = [False, False, False]
                self.limit_reset_until = None
                # Keep this phase active after driving reset low so the
                # realtime latch gets multiple servo cycles in which to
                # reassert if the physical input has returned.
                self.motion_not_before = now + 0.010
                self._message(
                    "bounce latch reset deasserted; validating cleared latch"
                )
                return
            if now < self.motion_not_before:
                return
            if any(safety_limits):
                return
            motor = self.collision_motor
            self.active = None
            self.pending = None
            self.phase = PHASE_IDLE
            self.collision_motor = None
            self.bounce_start_count = None
            self.phase_deadline = None
            self.interpreter.reset()
            self._message(
                f"bounce complete on motor {motor}; fresh pendant baseline required"
            )
            return

        if self.active is None:
            return
        stopped = machine.axis_stopped[self.active.axis_index]
        if not stopped or now < self.motion_not_before:
            return

        if self.phase == PHASE_STOPPING_BOUNCE:
            self._start_bounce_move(
                now=now,
                counts_by_motor=counts_by_motor,
                position_feedback_by_motor=position_feedback_by_motor,
                safety_limits=safety_limits,
            )
            return

        if self.phase == PHASE_BOUNCING:
            self._finish_bounce(
                now=now,
                counts_by_motor=counts_by_motor,
                raw_limits=raw_limits,
            )
            return

        self.active = None
        if self.phase == PHASE_STOPPING_REPLACE and self.pending is not None:
            request = self.pending
            self.pending = None
            self.phase = PHASE_IDLE
            self._start_jog(
                request,
                now=now,
                machine=machine,
                counts_by_motor=counts_by_motor,
                raw_limits=raw_limits,
                safety_limits=safety_limits,
            )
            return

        self.pending = None
        self.phase = PHASE_IDLE

    def _manage_homing_latches(
        self,
        *,
        now: float,
        machine: MachineSnapshot,
        raw_limits: tuple[bool, bool, bool],
        safety_limits: tuple[bool, bool, bool],
    ) -> None:
        if machine.any_homing:
            self.homing_was_active = True
            self._cancel_pendant_motion("homing is active")
            return
        if not self.homing_was_active:
            return
        if any(raw_limits):
            return

        if self.limit_reset_until is None and any(safety_limits):
            self.limit_reset = [bool(value) for value in safety_limits]
            self.limit_reset_until = now + 0.010
            return
        if self.limit_reset_until is not None and now >= self.limit_reset_until:
            self.limit_reset = [False, False, False]
            self.limit_reset_until = None
        if not any(safety_limits) and self.limit_reset_until is None:
            self.homing_was_active = False
            self._message("homing limit-event latches cleared")

    def update(
        self,
        *,
        now: float,
        link: LinkSnapshot,
        packet: PendantPacket | None,
        machine: MachineSnapshot,
        counts_by_motor: tuple[int, int, int],
        position_feedback_by_motor: tuple[float, float, float] | None = None,
        raw_limits: tuple[bool, bool, bool],
        safety_limits: tuple[bool, bool, bool],
        pendant_mode_enabled: bool = True,
    ) -> None:
        self._set_pendant_mode(pendant_mode_enabled)
        self.control_available = False
        self.control_ready = False

        if packet is not None:
            self.last_packet_at = now
            self.last_packet = packet
            self.link_established = True

        if self.faulted:
            self.external_enable = False
            self.control_ready = False
            return

        if not self.link_established:
            self.external_enable = False
            self.control_ready = False
            return

        if link.serial_fault or not link.connected:
            self._fault("Nano serial link fault after baseline")
            return
        if link.quadrature_fault:
            self._fault("Nano quadrature fault after baseline")
            return
        if (
            self.last_packet_at is None
            or now - self.last_packet_at > self.packet_timeout_seconds
        ):
            self._fault("Nano packet timeout after baseline")
            return

        if link.estop_pressed:
            if packet is not None and not self.recovery.active:
                self._engage_estop(packet)
            else:
                self.external_enable = False
                self.control_ready = False
            return

        if self.recovery_phase is not None:
            self._advance_recovery(
                now=now,
                machine=machine,
                link=link,
                raw_limits=raw_limits,
                safety_limits=safety_limits,
            )
            self.control_ready = False
            return

        if self.recovery.active:
            self.external_enable = False
            self.control_ready = False
            if packet is not None:
                update = self.recovery.process(packet)
                if update.message:
                    self._message(update.message)
                if update.unlock_requested:
                    self._begin_recovery_unlock(
                        now=now,
                        raw_limits=raw_limits,
                        safety_limits=safety_limits,
                        machine=machine,
                        packet=packet,
                    )
            return

        self._check_startup_limits(
            now=now,
            machine=machine,
            counts_by_motor=counts_by_motor,
            raw_limits=raw_limits,
            safety_limits=safety_limits,
        )
        if self.faulted:
            return

        if self.phase in STARTUP_POWER_PHASES:
            self.external_enable = True
            self._advance_startup_power(
                now=now,
                machine=machine,
                raw_limits=raw_limits,
                safety_limits=safety_limits,
            )
            self.control_ready = False
            return

        if self.phase in STARTUP_BOUNCE_PREP_PHASES:
            self.external_enable = True
            self._check_limits(
                now=now,
                machine=machine,
                safety_limits=safety_limits,
            )
            if self.faulted:
                return
            self._advance_startup_bounce(
                now=now,
                machine=machine,
                counts_by_motor=counts_by_motor,
                position_feedback_by_motor=position_feedback_by_motor,
                safety_limits=safety_limits,
            )
            self.control_ready = False
            return

        self.external_enable = True
        self._manage_homing_latches(
            now=now,
            machine=machine,
            raw_limits=raw_limits,
            safety_limits=safety_limits,
        )
        self._check_limits(
            now=now,
            machine=machine,
            safety_limits=safety_limits,
        )
        if self.faulted:
            return
        self._advance_motion(
            now=now,
            machine=machine,
            counts_by_motor=counts_by_motor,
            position_feedback_by_motor=position_feedback_by_motor,
            raw_limits=raw_limits,
            safety_limits=safety_limits,
        )
        if self.faulted:
            return

        self.control_available = (
            self.external_enable
            and self.startup_reset_complete
            and not self.faulted
            and not self.recovery_active
            and machine.ready_for_pendant_jog
            and (
                self.bounce_active
                or (not any(raw_limits) and not any(safety_limits))
            )
        )

        if not self.pendant_mode_enabled:
            return

        if packet is not None and not self.bounce_active:
            try:
                decision = self.interpreter.process(packet)
            except ControlFault as error:
                self._fault(str(error))
                return
            if decision.stop:
                self._cancel_pendant_motion(decision.reason)
            elif decision.jog is not None:
                self._request_jog(
                    decision.jog,
                    now=now,
                    machine=machine,
                    counts_by_motor=counts_by_motor,
                    raw_limits=raw_limits,
                    safety_limits=safety_limits,
                )

        self.control_ready = (
            self.control_available
            and self.pendant_mode_enabled
            and not self.bounce_active
            and not any(raw_limits)
            and not any(safety_limits)
        )
