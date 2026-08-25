from __future__ import annotations

import unittest
from dataclasses import replace

from tests.python import _support  # noqa: F401

from dmc2_reference.supervisor import (
    PHASE_BOUNCING,
    PHASE_BOUNCE_RESET,
    PHASE_STOPPING_BOUNCE,
    PHASE_STOPPING_REPLACE,
    PHASE_STARTUP_GATE_SETTLE,
    PHASE_STARTUP_READY_GATE_SETTLE,
    PHASE_STARTUP_READY_WAIT_ON,
    PHASE_STARTUP_READY_WAIT_RESET,
    PHASE_STARTUP_WAIT_ON,
    PHASE_STARTUP_WAIT_RESET,
    LinkSnapshot,
    LinuxCncPendantSupervisor,
    MachineSnapshot,
)
from pendant_protocol import PendantPacket


def packet(
    sequence: int,
    *,
    signal: int = 0,
    axis: str = "X",
    multiplier: str = "X1",
    deadman: bool = False,
    estop: bool = False,
    valid: bool = True,
    errors: int = 0,
) -> PendantPacket:
    return PendantPacket(
        sequence=sequence,
        milliseconds=sequence * 20,
        detent_count=sequence,
        transition_count=sequence * 4,
        quadrature_errors=errors,
        latest_detent_signal=signal,
        axis=axis,
        multiplier=multiplier,
        deadman_held=deadman,
        estop_pressed=estop,
        selector_valid=valid,
    )


READY_MACHINE = MachineSnapshot(
    machine_on=True,
    estopped=False,
    manual_mode=True,
    joint_mode=False,
    teleop_mode=True,
    interp_idle=True,
    homed=(True, True, True),
    homing=(False, False, False),
    axis_stopped=(True, True, True),
)


class FakeBackend:
    def __init__(self) -> None:
        self.commands: list[tuple] = []

    def stop_axis(self, axis_index: int, *, joint_jog: bool) -> None:
        self.commands.append(("stop", joint_jog, axis_index))

    def abort(self) -> None:
        self.commands.append(("abort",))

    def prepare_manual_teleop(self) -> None:
        self.commands.append(("manual-teleop",))

    def jog_increment(
        self,
        axis_index: int,
        signed_velocity: float,
        distance: float,
        *,
        joint_jog: bool,
    ) -> None:
        self.commands.append(
            ("jog", joint_jog, axis_index, signed_velocity, distance)
        )

    def request_estop_reset(self) -> None:
        self.commands.append(("estop-reset",))

    def request_machine_on(self) -> None:
        self.commands.append(("machine-on",))

    def clear_state_requests(self) -> None:
        pass


class SupervisorTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self.backend = FakeBackend()
        self.supervisor = LinuxCncPendantSupervisor(
            self.backend,
            pulses_per_mm=1000,
        )
        self.counts = (1000, 2000, 3000)
        self.position_feedback = (1.0, 2.0, 3.0)
        self.raw = (False, False, False)
        self.safety = (False, False, False)
        self.machine = READY_MACHINE

    def update(
        self,
        now: float,
        sample: PendantPacket | None,
        *,
        machine: MachineSnapshot | None = None,
        counts: tuple[int, int, int] | None = None,
        position_feedback: tuple[float, float, float] | None = None,
        raw: tuple[bool, bool, bool] | None = None,
        safety: tuple[bool, bool, bool] | None = None,
        serial_fault: bool = False,
        quadrature_fault: bool = False,
        pendant_mode_enabled: bool = True,
    ) -> None:
        current = sample or self.supervisor.last_packet
        self.assertIsNotNone(current)
        self.supervisor.update(
            now=now,
            link=LinkSnapshot(
                connected=not serial_fault,
                serial_fault=serial_fault,
                quadrature_fault=quadrature_fault,
                estop_pressed=bool(current.estop_pressed),
            ),
            packet=sample,
            machine=machine or self.machine,
            counts_by_motor=counts or self.counts,
            position_feedback_by_motor=(
                position_feedback or self.position_feedback
            ),
            raw_limits=raw or self.raw,
            safety_limits=safety or self.safety,
            pendant_mode_enabled=pendant_mode_enabled,
        )

    def establish_and_arm(self) -> None:
        self.update(0.000, packet(1))
        self.update(0.020, packet(2, deadman=True))
