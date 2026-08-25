from __future__ import annotations

import unittest
from dataclasses import replace

from live_motion_core import (
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


class SupervisorTests(unittest.TestCase):
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

    def test_mode_disabled_ignores_jogs_without_disabling_safety_chain(self):
        self.update(0.000, packet(1), pendant_mode_enabled=False)
        self.update(
            0.020,
            packet(2, deadman=True),
            pendant_mode_enabled=False,
        )
        self.update(
            0.040,
            packet(3, deadman=True, signal=1),
            pendant_mode_enabled=False,
        )

        self.assertFalse(any(command[0] == "jog" for command in self.backend.commands))
        self.assertTrue(self.supervisor.external_enable)
        self.assertFalse(self.supervisor.control_ready)
        self.assertFalse(self.supervisor.pendant_mode_enabled)

    def test_mode_enable_requires_a_fresh_deadman_baseline(self):
        self.update(0.000, packet(1), pendant_mode_enabled=False)
        self.update(
            0.020,
            packet(2, deadman=True, signal=1),
            pendant_mode_enabled=False,
        )
        self.update(0.040, packet(3), pendant_mode_enabled=True)
        self.update(0.060, packet(4, deadman=True), pendant_mode_enabled=True)
        self.assertFalse(any(command[0] == "jog" for command in self.backend.commands))

        self.update(
            0.080,
            packet(5, deadman=True, signal=1),
            pendant_mode_enabled=True,
        )
        self.assertEqual(self.backend.commands[-1], ("jog", False, 0, -5.0, 0.01))

    def test_mode_disable_stops_active_jog_and_discards_future_detents(self):
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=1), machine=moving)
        jog_count = sum(command[0] == "jog" for command in self.backend.commands)

        self.update(
            0.060,
            packet(4, deadman=True, signal=1),
            machine=moving,
            pendant_mode_enabled=False,
        )
        self.assertEqual(self.backend.commands[-1], ("stop", False, 0))
        self.assertIsNone(self.supervisor.pending)
        self.assertFalse(self.supervisor.control_ready)

        self.update(
            0.080,
            packet(5, deadman=True, signal=1),
            machine=moving,
            pendant_mode_enabled=False,
        )
        self.assertEqual(
            sum(command[0] == "jog" for command in self.backend.commands),
            jog_count,
        )

    def test_estop_is_processed_while_pendant_mode_is_disabled(self):
        self.update(0.000, packet(1), pendant_mode_enabled=False)
        self.update(
            0.020,
            packet(2, estop=True),
            pendant_mode_enabled=False,
        )

        self.assertTrue(self.supervisor.recovery.active)
        self.assertFalse(self.supervisor.external_enable)
        self.assertIn(("abort",), self.backend.commands)

    def test_limit_bounce_continues_after_mode_is_disabled(self):
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=-1), machine=moving)
        self.update(
            0.050,
            packet(4, deadman=True),
            machine=moving,
            raw=(False, True, False),
            safety=(False, True, False),
            pendant_mode_enabled=False,
        )

        self.assertTrue(self.supervisor.bounce_active)
        self.assertEqual(self.backend.commands[-2:], [("stop", False, 0), ("abort",)])

    def test_startup_limit_bounce_is_armed_with_pendant_mode_disabled(self):
        self.update(
            0.000,
            packet(1),
            raw=(False, True, False),
            safety=(False, True, False),
            pendant_mode_enabled=False,
        )

        self.assertTrue(self.supervisor.bounce_active)
        self.assertFalse(self.supervisor.pendant_mode_enabled)
        self.assertEqual(self.supervisor.active.motor, 1)
        self.assertEqual(self.supervisor.active.request.delta_pulses, -250)

    def test_clockwise_x_is_one_negative_ten_pulse_linuxcnc_jog(self):
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=1))

        self.assertEqual(self.backend.commands[-1], ("jog", False, 0, -5.0, 0.01))
        self.assertEqual(self.supervisor.active.target_count, 1990)
        self.assertTrue(self.supervisor.control_ready)

    def test_clockwise_y_and_z_keep_the_confirmed_positive_sign(self):
        self.update(0.000, packet(1, axis="Y", multiplier="X10"))
        self.update(
            0.020,
            packet(2, axis="Y", multiplier="X10", deadman=True),
        )
        self.update(
            0.040,
            packet(3, axis="Y", multiplier="X10", deadman=True, signal=1),
        )
        self.assertEqual(self.backend.commands[-1], ("jog", False, 1, 75.0, 0.1))

        stopped = replace(self.machine, axis_stopped=(True, True, True))
        self.update(0.100, packet(4, axis="Z", multiplier="X100"), machine=stopped)
        self.update(
            0.120,
            packet(5, axis="Z", multiplier="X100", deadman=True),
            machine=stopped,
        )
        self.update(
            0.140,
            packet(6, axis="Z", multiplier="X100", deadman=True, signal=1),
            machine=stopped,
        )
        self.assertEqual(self.backend.commands[-1], ("jog", False, 2, 300.0, 1.0))

    def test_unhomed_machine_uses_joint_jog_without_blocking_the_pendant(self):
        self.machine = replace(
            self.machine,
            joint_mode=True,
            teleop_mode=False,
            homed=(False, False, False),
        )
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=1))

        self.assertEqual(self.backend.commands[-1], ("jog", True, 0, -5.0, 0.01))
        self.assertTrue(self.supervisor.control_ready)

    def test_unhomed_limit_bounce_stays_in_joint_jog_mode(self):
        self.machine = replace(
            self.machine,
            joint_mode=True,
            teleop_mode=False,
            homed=(False, False, False),
        )
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=-1), machine=moving)
        self.update(
            0.050,
            packet(4, deadman=True),
            machine=moving,
            raw=(False, True, False),
            safety=(False, True, False),
        )
        self.assertEqual(self.backend.commands[-2:], [("stop", True, 0), ("abort",)])

        self.update(
            0.090,
            packet(5, deadman=True),
            raw=(False, True, False),
            safety=(False, True, False),
        )
        self.assertNotIn(("manual-teleop",), self.backend.commands)
        self.assertEqual(
            self.backend.commands[-1],
            ("jog", True, 0, -1.5, 0.2495),
        )

    def test_same_direction_replaces_target_without_stopping_or_queueing(self):
        self.machine = replace(
            self.machine,
            joint_mode=True,
            teleop_mode=False,
            homed=(False, False, False),
        )
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=1), machine=moving)
        self.assertEqual(self.supervisor.active.target_count, 1990)

        # Four of the original ten negative pulses have been generated.  The
        # old controller replaced its target with current-10 (1996-10=1986),
        # so LinuxCNC needs only a further four-pulse target extension.
        progressed = (1000, 1996, 3000)
        self.update(
            0.060,
            packet(4, deadman=True, signal=1),
            machine=moving,
            counts=progressed,
        )
        self.assertEqual(self.backend.commands[-1], ("jog", True, 0, -5.0, 0.004))
        self.assertEqual(self.supervisor.active.target_count, 1986)
        self.assertIsNone(self.supervisor.pending)
        self.assertNotIn(
            ("stop", True, 0),
            self.backend.commands,
        )

        # Another packet with no generated-count progress retains the same
        # target and emits no additional motion command.  It cannot grow a
        # hidden queue.
        command_count = len(self.backend.commands)
        self.update(
            0.080,
            packet(5, deadman=True, signal=1),
            machine=moving,
            counts=progressed,
        )
        self.assertEqual(len(self.backend.commands), command_count)
        self.assertEqual(self.supervisor.active.target_count, 1986)

    def test_positive_same_direction_extends_only_by_generated_progress(self):
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=-1), machine=moving)
        self.assertEqual(self.supervisor.active.target_count, 2010)

        progressed = (1000, 2007, 3000)
        self.update(
            0.060,
            packet(4, deadman=True, signal=-1),
            machine=moving,
            counts=progressed,
        )

        self.assertEqual(self.backend.commands[-1], ("jog", False, 0, 5.0, 0.007))
        self.assertEqual(self.supervisor.active.target_count, 2017)
        self.assertNotIn(("stop", False, 0), self.backend.commands)

    def test_direction_reversal_uses_one_replaceable_pending_slot(self):
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=1), machine=moving)
        self.update(0.060, packet(4, deadman=True, signal=-1), machine=moving)
        first_pending = self.supervisor.pending
        self.assertEqual(self.supervisor.phase, PHASE_STOPPING_REPLACE)
        self.update(0.080, packet(5, deadman=True, signal=1), machine=moving)

        self.assertIsNot(self.supervisor.pending, first_pending)
        self.assertEqual(self.supervisor.pending.delta_pulses, -10)
        self.assertEqual(
            [command for command in self.backend.commands if command[0] == "stop"],
            [("stop", False, 0)],
        )

        self.update(0.120, packet(6, deadman=True), machine=self.machine)
        self.assertEqual(self.backend.commands[-1], ("jog", False, 0, -5.0, 0.01))
        self.assertIsNone(self.supervisor.pending)

    def test_released_deadman_stops_active_jog(self):
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=1), machine=moving)
        self.update(0.060, packet(4, deadman=False), machine=moving)
        self.assertEqual(self.backend.commands[-1], ("stop", False, 0))

    def test_matching_positive_collision_runs_exact_negative_250_bounce(self):
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=-1), machine=moving)
        self.assertEqual(
            self.supervisor.command_enabled_by_motor,
            (False, True, False),
        )
        self.assertEqual(
            self.supervisor.toward_limit_by_motor,
            (False, True, False),
        )
        # Counterclockwise X is positive motor/joint motion toward IN11.
        self.update(
            0.050,
            packet(4, deadman=True),
            machine=moving,
            raw=(False, True, False),
            safety=(False, True, False),
        )
        self.assertEqual(self.supervisor.phase, PHASE_STOPPING_BOUNCE)
        self.assertTrue(self.supervisor.control_available)
        self.assertFalse(self.supervisor.control_ready)
        self.assertTrue(self.supervisor.pendant_mode_enabled)
        self.assertEqual(
            self.supervisor.command_enabled_by_motor,
            (False, False, False),
        )
        self.assertEqual(
            self.supervisor.toward_limit_by_motor,
            (False, False, False),
        )
        self.assertEqual(
            self.backend.commands[-2:],
            [("stop", False, 0), ("abort",)],
        )

        self.update(
            0.090,
            packet(5, deadman=True),
            raw=(False, True, False),
            safety=(False, True, False),
        )
        self.assertEqual(self.supervisor.phase, PHASE_BOUNCING)
        self.assertEqual(
            self.supervisor.command_enabled_by_motor,
            (False, True, False),
        )
        self.assertEqual(
            self.supervisor.toward_limit_by_motor,
            (False, False, False),
        )
        self.assertEqual(
            self.backend.commands[-2:],
            [
                ("manual-teleop",),
                ("jog", False, 0, -1.5, 0.2495),
            ],
        )

        self.update(
            0.300,
            packet(6, deadman=True),
            counts=(1000, 1750, 3000),
            raw=(False, False, False),
            safety=(False, True, False),
        )
        self.assertEqual(self.supervisor.phase, PHASE_BOUNCE_RESET)
        self.assertEqual(self.supervisor.limit_reset, [False, True, False])

        self.update(
            0.320,
            packet(7, deadman=True),
            counts=(1000, 1750, 3000),
            safety=(False, False, False),
        )
        self.assertTrue(self.supervisor.bounce_active)
        self.assertEqual(self.supervisor.limit_reset, [False, False, False])
        self.assertIsNone(self.supervisor.limit_reset_until)

        self.update(
            0.340,
            packet(8, deadman=True),
            counts=(1000, 1750, 3000),
            safety=(False, False, False),
        )
        self.assertFalse(self.supervisor.bounce_active)
        self.assertIsNone(self.supervisor.active)
        self.assertTrue(self.supervisor.control_available)
        self.assertTrue(self.supervisor.control_ready)

    def test_single_active_startup_limit_resets_powers_on_and_bounces_exactly(self):
        startup_machine = replace(
            self.machine,
            machine_on=False,
            estopped=True,
            joint_mode=True,
            teleop_mode=False,
            homed=(False, False, False),
        )
        x_limit = (False, True, False)

        self.update(
            0.000,
            packet(1),
            machine=startup_machine,
            raw=x_limit,
            safety=x_limit,
        )
        self.assertEqual(self.supervisor.phase, PHASE_STARTUP_GATE_SETTLE)
        self.assertTrue(self.supervisor.external_enable)
        self.assertTrue(self.supervisor.bounce_active)
        self.assertEqual(
            self.supervisor.command_enabled_by_motor,
            (False, True, False),
        )
        self.assertEqual(
            self.supervisor.toward_limit_by_motor,
            (False, False, False),
        )
        self.assertEqual(self.backend.commands, [])

        self.update(
            0.060,
            packet(2),
            machine=startup_machine,
            raw=x_limit,
            safety=x_limit,
        )
        self.assertEqual(self.supervisor.phase, PHASE_STARTUP_WAIT_RESET)
        self.assertEqual(self.backend.commands[-1], ("estop-reset",))

        reset_machine = replace(startup_machine, estopped=False)
        self.update(
            0.080,
            packet(3),
            machine=reset_machine,
            raw=x_limit,
            safety=x_limit,
        )
        self.assertEqual(self.supervisor.phase, PHASE_STARTUP_WAIT_ON)
        self.assertEqual(self.backend.commands[-1], ("machine-on",))

        running_machine = replace(reset_machine, machine_on=True)
        self.update(
            0.100,
            packet(4),
            machine=running_machine,
            raw=x_limit,
            safety=x_limit,
        )
        self.assertEqual(self.supervisor.phase, PHASE_BOUNCING)
        self.assertEqual(
            self.backend.commands[-1],
            ("jog", True, 0, -1.5, 0.2495),
        )

        self.update(
            0.300,
            packet(5),
            machine=running_machine,
            counts=(1000, 1750, 3000),
            raw=(False, False, False),
            safety=x_limit,
        )
        self.assertEqual(self.supervisor.phase, PHASE_BOUNCE_RESET)
        self.assertEqual(self.supervisor.limit_reset, [False, True, False])

        self.update(
            0.320,
            packet(6),
            machine=running_machine,
            counts=(1000, 1750, 3000),
            raw=(False, False, False),
            safety=(False, False, False),
        )
        self.assertTrue(self.supervisor.bounce_active)
        self.assertEqual(self.supervisor.limit_reset, [False, False, False])

        self.update(
            0.340,
            packet(7),
            machine=running_machine,
            counts=(1000, 1750, 3000),
            raw=(False, False, False),
            safety=(False, False, False),
        )
        self.assertFalse(self.supervisor.faulted)
        self.assertFalse(self.supervisor.bounce_active)
        self.assertIsNone(self.supervisor.active)

    def test_estop_during_latch_reset_forces_every_reset_output_low(self):
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=-1), machine=moving)
        self.update(
            0.050,
            packet(4, deadman=True),
            machine=moving,
            raw=(False, True, False),
            safety=(False, True, False),
        )
        self.update(
            0.090,
            packet(5, deadman=True),
            raw=(False, True, False),
            safety=(False, True, False),
        )
        self.update(
            0.300,
            packet(6, deadman=True),
            counts=(1000, 1750, 3000),
            raw=(False, False, False),
            safety=(False, True, False),
        )
        self.assertEqual(self.supervisor.limit_reset, [False, True, False])

        self.update(
            0.305,
            packet(7, estop=True),
            counts=(1000, 1750, 3000),
            raw=(False, False, False),
            safety=(False, False, False),
        )

        self.assertEqual(self.supervisor.limit_reset, [False, False, False])
        self.assertIsNone(self.supervisor.limit_reset_until)

    def test_fault_during_latch_reset_forces_every_reset_output_low(self):
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=-1), machine=moving)
        self.update(
            0.050,
            packet(4, deadman=True),
            machine=moving,
            raw=(False, True, False),
            safety=(False, True, False),
        )
        self.update(
            0.090,
            packet(5, deadman=True),
            raw=(False, True, False),
            safety=(False, True, False),
        )
        self.update(
            0.300,
            packet(6, deadman=True),
            counts=(1000, 1750, 3000),
            raw=(False, False, False),
            safety=(False, True, False),
        )
        self.assertEqual(self.supervisor.limit_reset, [False, True, False])

        self.update(
            0.305,
            packet(7, deadman=True),
            counts=(1000, 1750, 3000),
            raw=(False, False, False),
            safety=(False, False, False),
            serial_fault=True,
        )

        self.assertTrue(self.supervisor.faulted)
        self.assertEqual(self.supervisor.limit_reset, [False, False, False])
        self.assertIsNone(self.supervisor.limit_reset_until)

    def test_completed_latch_reset_can_attribute_the_next_limit_collision(self):
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=-1), machine=moving)
        self.update(
            0.050,
            packet(4, deadman=True),
            machine=moving,
            raw=(False, True, False),
            safety=(False, True, False),
        )
        self.update(
            0.090,
            packet(5, deadman=True),
            raw=(False, True, False),
            safety=(False, True, False),
        )
        self.update(
            0.300,
            packet(6, deadman=True),
            counts=(1000, 1750, 3000),
            raw=(False, False, False),
            safety=(False, True, False),
        )
        self.update(
            0.320,
            packet(7, deadman=True),
            counts=(1000, 1750, 3000),
            safety=(False, False, False),
        )
        self.update(
            0.340,
            packet(8, deadman=True),
            counts=(1000, 1750, 3000),
            safety=(False, False, False),
        )
        self.assertEqual(self.supervisor.limit_reset, [False, False, False])
        self.assertFalse(self.supervisor.bounce_active)

        self.update(
            0.360,
            packet(9, deadman=True),
            machine=moving,
            counts=(1000, 1750, 3000),
        )
        self.update(
            0.380,
            packet(10, deadman=True, signal=-1),
            machine=moving,
            counts=(1000, 1750, 3000),
        )
        self.update(
            0.390,
            packet(11, deadman=True),
            machine=moving,
            counts=(1000, 1750, 3000),
            raw=(False, True, False),
            safety=(False, True, False),
        )

        self.assertFalse(self.supervisor.faulted)
        self.assertEqual(self.supervisor.phase, PHASE_STOPPING_BOUNCE)
        self.assertTrue(self.supervisor.control_available)
        self.assertTrue(self.supervisor.pendant_mode_enabled)

    def test_bounce_targets_exact_count_from_live_negative_fractional_phase(self):
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=-1), machine=moving)
        self.update(
            0.050,
            packet(4, deadman=True),
            machine=moving,
            raw=(False, True, False),
            safety=(False, True, False),
        )

        live_counts = (1000, -251, 3000)
        live_position = (1.0, -0.2500000457763672, 3.0)
        self.update(
            0.090,
            packet(5, deadman=True),
            counts=live_counts,
            position_feedback=live_position,
            raw=(False, True, False),
            safety=(False, True, False),
        )

        command = self.backend.commands[-1]
        self.assertEqual(command[:4], ("jog", False, 0, -1.5))
        self.assertAlmostEqual(command[4], 0.2504999542236328)
        self.assertEqual(self.supervisor.bounce_start_count, -251)

        self.update(
            0.300,
            packet(6, deadman=True),
            counts=(1000, -501, 3000),
            position_feedback=(1.0, -0.5005, 3.0),
            raw=(False, False, False),
            safety=(False, True, False),
        )
        self.assertFalse(self.supervisor.faulted)
        self.assertEqual(self.supervisor.phase, PHASE_BOUNCE_RESET)
        self.assertEqual(self.supervisor.limit_reset, [False, True, False])

    def test_bounce_refuses_incoherent_fractional_feedback_before_command(self):
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=-1), machine=moving)
        self.update(
            0.050,
            packet(4, deadman=True),
            machine=moving,
            raw=(False, True, False),
            safety=(False, True, False),
        )
        self.update(
            0.090,
            packet(5, deadman=True),
            position_feedback=(1.0, float("nan"), 3.0),
            raw=(False, True, False),
            safety=(False, True, False),
        )

        self.assertTrue(self.supervisor.faulted)
        self.assertIn("count/position feedback was incoherent", self.supervisor.fault)
        self.assertNotIn(("manual-teleop",), self.backend.commands)
        self.assertEqual(
            sum(command[0] == "jog" for command in self.backend.commands),
            1,
        )

    def test_clean_startup_resets_estop_and_turns_machine_on_automatically(self):
        startup_machine = replace(
            self.machine,
            machine_on=False,
            estopped=True,
            joint_mode=True,
            teleop_mode=False,
            homed=(False, False, False),
        )

        self.update(0.000, packet(1), machine=startup_machine)
        self.assertEqual(
            self.supervisor.phase,
            PHASE_STARTUP_READY_GATE_SETTLE,
        )
        self.assertTrue(self.supervisor.external_enable)
        self.assertEqual(self.backend.commands, [])

        self.update(0.060, packet(2), machine=startup_machine)
        self.assertEqual(
            self.supervisor.phase,
            PHASE_STARTUP_READY_WAIT_RESET,
        )
        self.assertEqual(self.backend.commands[-1], ("estop-reset",))

        reset_machine = replace(startup_machine, estopped=False)
        self.update(0.080, packet(3), machine=reset_machine)
        self.assertEqual(
            self.supervisor.phase,
            PHASE_STARTUP_READY_WAIT_ON,
        )
        self.assertEqual(self.backend.commands[-1], ("machine-on",))

        running_machine = replace(reset_machine, machine_on=True)
        self.update(0.100, packet(4), machine=running_machine)
        self.assertEqual(self.supervisor.phase, "idle")
        self.assertTrue(self.supervisor.startup_reset_complete)
        self.assertFalse(self.supervisor.faulted)
        self.assertFalse(any(command[0] == "jog" for command in self.backend.commands))

        self.update(0.120, packet(5), machine=running_machine)
        self.assertTrue(self.supervisor.control_ready)

    def test_automatic_startup_reset_waits_for_events_without_time_limit(self):
        startup_machine = replace(
            self.machine,
            machine_on=False,
            estopped=True,
            joint_mode=True,
            teleop_mode=False,
            homed=(False, False, False),
        )

        self.update(0.000, packet(1), machine=startup_machine)
        self.update(300.000, packet(2), machine=startup_machine)
        self.assertEqual(self.supervisor.phase, PHASE_STARTUP_READY_WAIT_RESET)
        self.assertFalse(self.supervisor.faulted)

        self.update(600.000, packet(3), machine=startup_machine)
        self.assertEqual(self.supervisor.phase, PHASE_STARTUP_READY_WAIT_RESET)
        self.assertFalse(self.supervisor.faulted)

        reset_machine = replace(startup_machine, estopped=False)
        self.update(600.020, packet(4), machine=reset_machine)
        self.assertEqual(self.supervisor.phase, PHASE_STARTUP_READY_WAIT_ON)

        self.update(900.000, packet(5), machine=reset_machine)
        self.assertEqual(self.supervisor.phase, PHASE_STARTUP_READY_WAIT_ON)
        self.assertFalse(self.supervisor.faulted)

        running_machine = replace(reset_machine, machine_on=True)
        self.update(900.020, packet(6), machine=running_machine)
        self.assertTrue(self.supervisor.startup_reset_complete)
        self.assertFalse(self.supervisor.faulted)

    def test_disabling_pendant_mode_does_not_cancel_startup_reset_phase(self):
        startup_machine = replace(
            self.machine,
            machine_on=False,
            estopped=True,
            joint_mode=True,
            teleop_mode=False,
            homed=(False, False, False),
        )

        self.update(
            0.000,
            packet(1),
            machine=startup_machine,
            pendant_mode_enabled=True,
        )
        self.assertEqual(self.supervisor.phase, PHASE_STARTUP_READY_GATE_SETTLE)

        self.update(
            0.010,
            packet(2),
            machine=startup_machine,
            pendant_mode_enabled=False,
        )
        self.assertEqual(self.supervisor.phase, PHASE_STARTUP_READY_GATE_SETTLE)

        self.update(
            0.060,
            packet(3),
            machine=startup_machine,
            pendant_mode_enabled=False,
        )
        self.assertEqual(self.supervisor.phase, PHASE_STARTUP_READY_WAIT_RESET)
        self.assertEqual(self.backend.commands[-1], ("estop-reset",))

    def test_control_available_precedes_acknowledged_pendant_ready(self):
        self.update(
            0.000,
            packet(1),
            machine=self.machine,
            pendant_mode_enabled=False,
        )
        self.assertTrue(self.supervisor.control_available)
        self.assertFalse(self.supervisor.control_ready)

        self.update(
            0.020,
            packet(2),
            machine=self.machine,
            pendant_mode_enabled=True,
        )
        self.assertTrue(self.supervisor.control_available)
        self.assertTrue(self.supervisor.control_ready)

    def test_watchdog_rearm_restarts_uncommitted_automatic_reset(self):
        startup_machine = replace(
            self.machine,
            machine_on=False,
            estopped=True,
            joint_mode=True,
            teleop_mode=False,
            homed=(False, False, False),
        )

        self.update(0.000, packet(1), machine=startup_machine)
        self.update(0.060, packet(2), machine=startup_machine)
        self.assertEqual(self.supervisor.phase, PHASE_STARTUP_READY_WAIT_RESET)

        self.supervisor.restart_uncommitted_startup(now=300.000)
        self.assertEqual(
            self.supervisor.phase,
            PHASE_STARTUP_READY_GATE_SETTLE,
        )
        self.assertFalse(self.supervisor.external_enable)
        self.assertFalse(self.supervisor.faulted)

        self.update(300.060, packet(3), machine=startup_machine)
        self.assertEqual(self.supervisor.phase, PHASE_STARTUP_READY_WAIT_RESET)
        reset_commands = [
            command
            for command in self.backend.commands
            if command == ("estop-reset",)
        ]
        self.assertEqual(
            reset_commands,
            [("estop-reset",), ("estop-reset",)],
        )

    def test_startup_bounce_maps_each_single_limit_to_its_existing_axis(self):
        expected_axis = {0: 1, 1: 0, 2: 2}
        for motor in range(3):
            with self.subTest(motor=motor):
                backend = FakeBackend()
                supervisor = LinuxCncPendantSupervisor(
                    backend,
                    pulses_per_mm=1000,
                )
                limit = tuple(index == motor for index in range(3))
                supervisor.update(
                    now=0.000,
                    link=LinkSnapshot(
                        connected=True,
                        serial_fault=False,
                        quadrature_fault=False,
                        estop_pressed=False,
                    ),
                    packet=packet(1),
                    machine=READY_MACHINE,
                    counts_by_motor=self.counts,
                    position_feedback_by_motor=self.position_feedback,
                    raw_limits=limit,
                    safety_limits=limit,
                )
                supervisor.update(
                    now=0.060,
                    link=LinkSnapshot(
                        connected=True,
                        serial_fault=False,
                        quadrature_fault=False,
                        estop_pressed=False,
                    ),
                    packet=packet(2),
                    machine=READY_MACHINE,
                    counts_by_motor=self.counts,
                    position_feedback_by_motor=self.position_feedback,
                    raw_limits=limit,
                    safety_limits=limit,
                )
                self.assertEqual(supervisor.phase, PHASE_BOUNCING)
                self.assertEqual(
                    backend.commands[-1],
                    ("jog", False, expected_axis[motor], -1.5, 0.2495),
                )

    def test_startup_raw_limit_waits_for_its_realtime_latch(self):
        x_raw = (False, True, False)
        self.update(
            0.000,
            packet(1),
            raw=x_raw,
            safety=(False, False, False),
        )
        self.assertEqual(self.supervisor.phase, PHASE_STARTUP_GATE_SETTLE)
        self.assertEqual(self.backend.commands, [])

        self.update(
            0.060,
            packet(2),
            raw=x_raw,
            safety=x_raw,
        )
        self.assertEqual(self.supervisor.phase, PHASE_BOUNCING)
        self.assertEqual(
            self.backend.commands[-1],
            ("jog", False, 0, -1.5, 0.2495),
        )

    def test_multiple_or_mismatched_startup_limits_fault_without_motion(self):
        cases = (
            ((True, True, False), (True, True, False)),
            ((False, True, False), (True, False, False)),
        )
        for raw, safety in cases:
            with self.subTest(raw=raw, safety=safety):
                backend = FakeBackend()
                supervisor = LinuxCncPendantSupervisor(
                    backend,
                    pulses_per_mm=1000,
                )
                supervisor.update(
                    now=0.000,
                    link=LinkSnapshot(
                        connected=True,
                        serial_fault=False,
                        quadrature_fault=False,
                        estop_pressed=False,
                    ),
                    packet=packet(1),
                    machine=READY_MACHINE,
                    counts_by_motor=self.counts,
                    position_feedback_by_motor=self.position_feedback,
                    raw_limits=raw,
                    safety_limits=safety,
                )
                self.assertTrue(supervisor.faulted)
                self.assertFalse(any(command[0] == "jog" for command in backend.commands))

    def test_249_pulse_bounce_fails_closed(self):
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=-1), machine=moving)
        self.update(
            0.050,
            packet(4, deadman=True),
            machine=moving,
            safety=(False, True, False),
        )
        self.update(
            0.090,
            packet(5, deadman=True),
            safety=(False, True, False),
        )
        self.update(
            0.300,
            packet(6, deadman=True),
            counts=(1000, 1751, 3000),
            safety=(False, True, False),
        )
        self.assertTrue(self.supervisor.faulted)
        self.assertFalse(self.supervisor.external_enable)
        self.assertIn("required -250, generated -249", self.supervisor.fault)

    def test_wrong_limit_during_pendant_jog_fails_without_bounce(self):
        moving = replace(self.machine, axis_stopped=(False, True, True))
        self.establish_and_arm()
        self.update(0.040, packet(3, deadman=True, signal=-1), machine=moving)
        self.update(
            0.050,
            packet(4, deadman=True),
            machine=moving,
            safety=(True, False, False),
        )
        self.assertTrue(self.supervisor.faulted)
        self.assertFalse(
            any(command[0] == "jog" for command in self.backend.commands[1:])
        )

    def test_homing_limit_events_are_ignored_then_realtime_latches_reset(self):
        self.update(0.000, packet(1))
        homing = replace(
            self.machine,
            homed=(False, False, False),
            homing=(True, False, False),
        )
        self.update(
            0.020,
            packet(2),
            machine=homing,
            raw=(False, True, False),
            safety=(False, True, False),
        )
        self.assertFalse(self.supervisor.faulted)

        unhomed = replace(
            self.machine,
            homed=(False, False, False),
            homing=(False, False, False),
        )
        self.update(
            0.040,
            packet(3),
            machine=unhomed,
            raw=(False, False, False),
            safety=(False, True, False),
        )
        self.assertEqual(self.supervisor.limit_reset, [False, True, False])
        self.update(
            0.060,
            packet(4),
            machine=unhomed,
            safety=(False, False, False),
        )
        self.assertFalse(self.supervisor.homing_was_active)
        self.assertFalse(self.supervisor.faulted)

    def test_packet_timeout_after_baseline_fails_closed(self):
        self.update(0.000, packet(1))
        self.update(0.101, None)
        self.assertTrue(self.supervisor.faulted)
        self.assertFalse(self.supervisor.external_enable)

    def test_estop_unlock_sequence_is_input_only_then_resets_and_turns_on(self):
        self.update(0.000, packet(1))
        self.update(0.020, packet(2, estop=True))
        self.assertTrue(self.supervisor.recovery_active)
        self.assertFalse(self.supervisor.external_enable)

        self.machine = replace(self.machine, machine_on=False, estopped=True)
        sequence = [
            packet(3, axis="X", multiplier="X10"),
            packet(4, axis="X", multiplier="X1"),
            packet(5, axis="N", multiplier="N", valid=False),
            packet(6, axis="N", multiplier="N", valid=False, signal=1),
            packet(7, axis="N", multiplier="N", valid=False, signal=-1),
            packet(8, axis="N", multiplier="X1", valid=False, deadman=True),
            packet(9, axis="N", multiplier="N", valid=False),
            packet(10, axis="N", multiplier="X1", valid=False, deadman=True),
            packet(11, axis="N", multiplier="N", valid=False),
            packet(12, axis="N", multiplier="X1", valid=False, deadman=True),
            packet(13, axis="N", multiplier="N", valid=False),
        ]
        now = 0.040
        for sample in sequence:
            self.update(now, sample)
            now += 0.020

        self.assertTrue(self.supervisor.external_enable)
        self.assertTrue(self.supervisor.recovery_active)
        self.assertFalse(any(command[0] == "jog" for command in self.backend.commands))

        self.update(now + 0.060, packet(14, axis="N", multiplier="N", valid=False))
        self.assertIn(("estop-reset",), self.backend.commands)
        self.machine = replace(self.machine, estopped=False)
        self.update(now + 0.080, packet(15, axis="N", multiplier="N", valid=False))
        self.assertIn(("machine-on",), self.backend.commands)
        self.machine = replace(self.machine, machine_on=True)
        self.update(now + 0.100, packet(16, axis="N", multiplier="N", valid=False))
        self.assertFalse(self.supervisor.recovery_active)


if __name__ == "__main__":
    unittest.main()
