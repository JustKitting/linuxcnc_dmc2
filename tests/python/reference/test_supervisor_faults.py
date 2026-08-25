"""FaultAndRecovery supervisor regression cases."""

from .supervisor_harness import *  # noqa: F403


class FaultAndRecoveryTests(SupervisorTestCase):  # noqa: F405
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
