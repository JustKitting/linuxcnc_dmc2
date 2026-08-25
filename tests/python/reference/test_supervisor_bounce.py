"""Bounce supervisor regression cases."""

from .supervisor_harness import *  # noqa: F403


class BounceTests(SupervisorTestCase):  # noqa: F405
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
