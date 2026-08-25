import unittest

from control import ActiveMotion, PendantMachineController
from control_core import (
    BOUNCE_RATE_PULSES_PER_SECOND,
    JOG_RATE_BY_MULTIPLIER,
    JOG_RATE_PULSES_PER_SECOND,
    RECOVERY_WAIT_BUTTON_PRESS,
    RECOVERY_WAIT_CLOCKWISE,
    RECOVERY_WAIT_OFF,
    RECOVERY_WAIT_X1,
    RECOVERY_WAIT_X10,
    ControlFault,
    JogRequest,
)
from pendant_protocol import PendantPacket


def recovery_packet(
    sequence,
    *,
    axis="N",
    multiplier="N",
    signal=0,
    deadman=False,
    estop=False,
    valid=False,
    errors=0,
):
    return PendantPacket(
        sequence=sequence,
        milliseconds=sequence * 20,
        detent_count=0,
        transition_count=0,
        quadrature_errors=errors,
        latest_detent_signal=signal,
        axis=axis,
        multiplier=multiplier,
        deadman_held=deadman,
        estop_pressed=estop,
        selector_valid=valid,
    )


class FakeHardware:
    def __init__(self):
        self.jog_rate = JOG_RATE_PULSES_PER_SECOND
        self.counts = [1000, 2000, 3000]
        self.raw = [False, False, False]
        self.latched = [False, False, False]
        self.commands = [False, False, False]
        self.toward = [False, False, False]
        self.positions = list(self.counts)
        self.rates = [self.jog_rate, self.jog_rate, self.jog_rate]
        self.watchdog = True
        self.reset_calls = []

    def count(self, motor):
        return self.counts[motor]

    def raw_limits(self):
        return tuple(self.raw)

    def latched_limits(self):
        return tuple(self.latched)

    def watchdog_ok(self):
        return self.watchdog

    def guard_enabled(self, motor):
        if not self.commands[motor] or not self.watchdog:
            return False
        if any(self.latched[other] for other in range(3) if other != motor):
            return False
        return not (self.latched[motor] and self.toward[motor])

    def set_position(self, motor, count):
        self.positions[motor] = count

    def set_command(self, motor, enabled):
        self.commands[motor] = enabled

    def set_toward(self, motor, toward):
        self.toward[motor] = toward

    def set_rate(self, motor, rate):
        self.rates[motor] = rate

    def disable_commands(self):
        self.commands = [False, False, False]

    def cancel_and_align(self):
        self.disable_commands()
        self.positions = list(self.counts)
        self.toward = [False, False, False]
        self.rates = [self.jog_rate, self.jog_rate, self.jog_rate]
        return tuple(self.counts)

    def reset_limit_latches(self, motors):
        self.reset_calls.append(tuple(motors))
        for motor in motors:
            if not self.raw[motor]:
                self.latched[motor] = False


class RuntimeLimitBounceTests(unittest.TestCase):
    def controller(self):
        hardware = FakeHardware()
        controller = PendantMachineController(
            hardware=hardware,
        )
        return controller, hardware

    def test_collision_replaces_jog_with_one_exact_negative_250_target(self):
        controller, hardware = self.controller()
        hardware.commands[1] = True
        hardware.toward[1] = True
        controller.active = ActiveMotion(
            mode="jog",
            motor=1,
            target_count=9000,
            toward_limit=True,
            start_count=2000,
        )
        hardware.latched[1] = True

        controller.poll_motion()

        self.assertEqual(controller.active.mode, "bounce")
        self.assertEqual(controller.active.motor, 1)
        self.assertEqual(controller.active.start_count, 2000)
        self.assertEqual(controller.active.target_count, 1750)
        self.assertEqual(hardware.positions[1], 1750)
        self.assertEqual(hardware.rates[1], BOUNCE_RATE_PULSES_PER_SECOND)
        self.assertEqual(hardware.commands, [False, True, False])
        self.assertFalse(hardware.toward[1])

    def test_exact_250_completion_resets_latch_and_control_baseline(self):
        controller, hardware = self.controller()
        hardware.latched[1] = True
        hardware.commands[1] = True
        hardware.toward[1] = False
        hardware.counts[1] = 1750
        controller.active = ActiveMotion(
            mode="bounce",
            motor=1,
            target_count=1750,
            toward_limit=False,
            start_count=2000,
        )

        controller.poll_motion()

        self.assertIsNone(controller.active)
        self.assertEqual(hardware.reset_calls, [(1,)])
        self.assertFalse(hardware.latched[1])
        self.assertFalse(hardware.commands[1])
        self.assertEqual(hardware.rates[1], hardware.jog_rate)

    def test_249_counts_is_not_accepted_as_a_250_count_bounce(self):
        controller, hardware = self.controller()
        hardware.latched[1] = True
        hardware.commands[1] = True
        hardware.counts[1] = 1751
        controller.active = ActiveMotion(
            mode="bounce",
            motor=1,
            target_count=1750,
            toward_limit=False,
            start_count=2000,
        )

        controller.poll_motion()

        self.assertIsNotNone(controller.active)
        self.assertEqual(hardware.reset_calls, [])

    def test_wrong_limit_does_not_move_a_different_axis(self):
        controller, hardware = self.controller()
        hardware.commands[1] = True
        hardware.toward[1] = True
        controller.active = ActiveMotion(
            mode="jog",
            motor=1,
            target_count=9000,
            toward_limit=True,
            start_count=2000,
        )
        hardware.latched[0] = True

        with self.assertRaises(ControlFault):
            controller.poll_motion()
        self.assertEqual(hardware.commands, [False, False, False])

    def test_new_poll_replaces_target_instead_of_adding_to_queue(self):
        controller, hardware = self.controller()
        hardware.counts[1] = 1995
        hardware.commands[1] = True
        hardware.toward[1] = False
        controller.active = ActiveMotion(
            mode="jog",
            motor=1,
            target_count=1990,
            toward_limit=False,
            start_count=2000,
        )

        controller.apply_jog(
            JogRequest(
                motor=1,
                delta_pulses=-10,
                axis="X",
                multiplier="X1",
                detent_delta=1,
            )
        )

        self.assertEqual(controller.active.target_count, 1985)
        self.assertEqual(hardware.positions[1], 1985)
        self.assertNotEqual(controller.active.target_count, 1980)
        self.assertEqual(hardware.rates[1], JOG_RATE_BY_MULTIPLIER["X1"])

    def test_x1_starts_at_exactly_5000_steps_per_second(self):
        controller, hardware = self.controller()

        controller.apply_jog(
            JogRequest(
                motor=1,
                delta_pulses=-10,
                axis="X",
                multiplier="X1",
                detent_delta=1,
            )
        )

        self.assertEqual(hardware.positions[1], 1990)
        self.assertEqual(hardware.rates[1], 5000.0)
        self.assertTrue(hardware.commands[1])


class RuntimeEstopRecoveryTests(unittest.TestCase):
    def controller(self):
        hardware = FakeHardware()
        controller = PendantMachineController(hardware=hardware)
        return controller, hardware

    @staticmethod
    def assert_disabled(test_case, hardware):
        test_case.assertEqual(hardware.commands, [False, False, False])

    def begin_through_off(self, controller, hardware):
        hardware.commands[1] = True
        controller.active = ActiveMotion(
            mode="jog",
            motor=1,
            target_count=2100,
            toward_limit=True,
            start_count=2000,
        )
        controller.process_packet(
            recovery_packet(
                1,
                axis="X",
                multiplier="X1",
                estop=True,
                valid=True,
            )
        )
        self.assertTrue(controller.estop_recovery.active)
        self.assertIsNone(controller.active)
        self.assert_disabled(self, hardware)

        controller.process_packet(
            recovery_packet(
                2,
                axis="X",
                multiplier="X10",
                valid=True,
            )
        )
        self.assertEqual(controller.estop_recovery.stage, RECOVERY_WAIT_X1)
        self.assert_disabled(self, hardware)

        controller.process_packet(
            recovery_packet(
                3,
                axis="X",
                multiplier="X1",
                valid=True,
            )
        )
        self.assertEqual(controller.estop_recovery.stage, RECOVERY_WAIT_OFF)
        self.assert_disabled(self, hardware)

        controller.process_packet(recovery_packet(4))
        self.assertEqual(controller.estop_recovery.stage, RECOVERY_WAIT_CLOCKWISE)
        self.assert_disabled(self, hardware)

    def test_exact_sequence_unlocks_after_third_complete_click_without_motion(self):
        controller, hardware = self.controller()
        self.begin_through_off(controller, hardware)

        controller.process_packet(recovery_packet(5, signal=+1))
        self.assert_disabled(self, hardware)
        controller.process_packet(recovery_packet(6, signal=+1))
        self.assert_disabled(self, hardware)
        controller.process_packet(recovery_packet(7, signal=-1))
        self.assertEqual(
            controller.estop_recovery.stage, RECOVERY_WAIT_BUTTON_PRESS
        )
        self.assert_disabled(self, hardware)

        sequence = 8
        for completed_clicks in range(3):
            controller.process_packet(
                recovery_packet(
                    sequence,
                    multiplier="X1",
                    deadman=True,
                )
            )
            self.assertTrue(controller.estop_recovery.active)
            self.assertEqual(controller.estop_recovery.clicks, completed_clicks)
            self.assert_disabled(self, hardware)
            sequence += 1

            controller.process_packet(recovery_packet(sequence))
            self.assert_disabled(self, hardware)
            sequence += 1

        self.assertFalse(controller.estop_recovery.active)
        self.assertEqual(controller.estop_recovery.clicks, 0)
        self.assertIsNone(controller.interpreter.previous)
        self.assert_disabled(self, hardware)

    def test_counterclockwise_before_clockwise_restarts_at_x_plus_x10(self):
        controller, hardware = self.controller()
        self.begin_through_off(controller, hardware)

        controller.process_packet(recovery_packet(5, signal=-1))

        self.assertEqual(controller.estop_recovery.stage, RECOVERY_WAIT_X10)
        self.assert_disabled(self, hardware)

    def test_first_button_press_must_expose_x1_while_axis_is_off(self):
        controller, hardware = self.controller()
        self.begin_through_off(controller, hardware)
        controller.process_packet(recovery_packet(5, signal=+1))
        controller.process_packet(recovery_packet(6, signal=-1))

        controller.process_packet(
            recovery_packet(7, multiplier="X10", deadman=True)
        )

        self.assertEqual(controller.estop_recovery.stage, RECOVERY_WAIT_X10)
        self.assert_disabled(self, hardware)

    def test_x1_does_not_count_until_x10_was_observed_first(self):
        controller, hardware = self.controller()
        controller.process_packet(recovery_packet(1, estop=True))
        controller.process_packet(
            recovery_packet(
                2,
                axis="X",
                multiplier="X1",
                valid=True,
            )
        )

        self.assertEqual(controller.estop_recovery.stage, RECOVERY_WAIT_X10)
        self.assert_disabled(self, hardware)

    def test_off_before_x1_restarts_at_x_plus_x10(self):
        controller, hardware = self.controller()
        controller.process_packet(recovery_packet(1, estop=True))
        controller.process_packet(
            recovery_packet(
                2,
                axis="X",
                multiplier="X10",
                valid=True,
            )
        )
        self.assertEqual(controller.estop_recovery.stage, RECOVERY_WAIT_X1)

        controller.process_packet(recovery_packet(3))

        self.assertEqual(controller.estop_recovery.stage, RECOVERY_WAIT_X10)
        self.assert_disabled(self, hardware)

    def test_repressing_estop_resets_the_sequence_and_never_exits(self):
        controller, hardware = self.controller()
        self.begin_through_off(controller, hardware)
        controller.process_packet(recovery_packet(5, signal=+1))

        controller.process_packet(recovery_packet(6, estop=True))
        self.assertTrue(controller.estop_recovery.active)
        self.assert_disabled(self, hardware)
        controller.process_packet(
            recovery_packet(
                7,
                axis="X",
                multiplier="X10",
                valid=True,
            )
        )

        self.assertEqual(controller.estop_recovery.stage, RECOVERY_WAIT_X1)
        self.assert_disabled(self, hardware)

    def test_unlock_is_refused_while_a_limit_is_active(self):
        controller, hardware = self.controller()
        self.begin_through_off(controller, hardware)
        controller.process_packet(recovery_packet(5, signal=+1))
        controller.process_packet(recovery_packet(6, signal=-1))
        sequence = 7
        for _ in range(2):
            controller.process_packet(
                recovery_packet(sequence, multiplier="X1", deadman=True)
            )
            sequence += 1
            controller.process_packet(recovery_packet(sequence))
            sequence += 1
        controller.process_packet(
            recovery_packet(sequence, multiplier="X1", deadman=True)
        )
        hardware.raw[1] = True
        controller.process_packet(recovery_packet(sequence + 1))

        self.assertTrue(controller.estop_recovery.active)
        self.assertEqual(controller.estop_recovery.stage, RECOVERY_WAIT_X10)
        self.assert_disabled(self, hardware)


if __name__ == "__main__":
    unittest.main()
