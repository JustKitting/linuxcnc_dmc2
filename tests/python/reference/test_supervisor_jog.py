"""Jog supervisor regression cases."""

from .supervisor_harness import *  # noqa: F403


class JogTests(SupervisorTestCase):  # noqa: F405
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
