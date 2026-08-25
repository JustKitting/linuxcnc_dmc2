"""Startup supervisor regression cases."""

from .supervisor_harness import *  # noqa: F403


class StartupTests(SupervisorTestCase):  # noqa: F405
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
