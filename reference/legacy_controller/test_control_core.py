import unittest

from control_core import (
    BOUNCE_PULSES,
    BOUNCE_RATE_PULSES_PER_SECOND,
    CLOCKWISE_SIGN_BY_AXIS,
    JOG_RATE_BY_MULTIPLIER,
    JOG_RATE_PULSES_PER_SECOND,
    JOG_RATE_SCALE_BY_MULTIPLIER,
    PREVIOUS_JOG_RATE_BY_MULTIPLIER,
    ControlFault,
    PendantInterpreter,
    decide_limit_action,
    make_bounce_plan,
)
from motion_config import (
    MOTOR_PULSES_PER_REV,
    PULSE_SCALE,
    REFERENCE_PULSES_PER_REV,
)
from pendant_protocol import PendantPacket


def packet(
    *,
    sequence=1,
    detents=0,
    errors=0,
    axis="X",
    multiplier="X1",
    signal=None,
    deadman=True,
    estop=False,
    valid=True,
):
    if signal is None:
        signal = 0 if detents == 0 else (1 if detents > 0 else -1)
    return PendantPacket(
        sequence=sequence,
        milliseconds=sequence * 20,
        detent_count=detents,
        transition_count=detents * 4,
        quadrature_errors=errors,
        latest_detent_signal=signal,
        axis=axis,
        multiplier=multiplier,
        deadman_held=deadman,
        estop_pressed=estop,
        selector_valid=valid,
    )


class InterpreterTests(unittest.TestCase):
    def arm(self, interpreter, axis="X", multiplier="X1"):
        self.assertTrue(
            interpreter.process(
                packet(
                    sequence=1,
                    axis=axis,
                    multiplier=multiplier,
                    deadman=False,
                )
            ).stop
        )
        self.assertTrue(
            interpreter.process(
                packet(sequence=2, axis=axis, multiplier=multiplier, deadman=True)
            ).stop
        )

    def test_x1_is_ten_pulses_per_detent(self):
        interpreter = PendantInterpreter()
        self.arm(interpreter)
        decision = interpreter.process(packet(sequence=3, detents=1))
        self.assertEqual(decision.jog.motor, 1)
        self.assertEqual(abs(decision.jog.delta_pulses), 10)

    def test_multiplier_scaling(self):
        for multiplier, expected in (("X1", -10), ("X10", -100), ("X100", -1000)):
            with self.subTest(multiplier=multiplier):
                interpreter = PendantInterpreter()
                self.arm(interpreter, multiplier=multiplier)
                decision = interpreter.process(
                    packet(sequence=3, detents=1, multiplier=multiplier)
                )
                self.assertEqual(decision.jog.delta_pulses, expected)

    def test_axis_to_motor_mapping(self):
        for axis, expected_motor in (("X", 1), ("Y", 0), ("Z", 2)):
            with self.subTest(axis=axis):
                interpreter = PendantInterpreter()
                self.arm(interpreter, axis=axis)
                decision = interpreter.process(packet(sequence=3, detents=1, axis=axis))
                self.assertEqual(decision.jog.motor, expected_motor)

    def test_user_confirmed_clockwise_direction_is_axis_specific(self):
        self.assertEqual(CLOCKWISE_SIGN_BY_AXIS, {"X": -1, "Y": 1, "Z": 1})
        for axis, expected in (("X", -10), ("Y", 10), ("Z", 10)):
            with self.subTest(axis=axis):
                interpreter = PendantInterpreter()
                self.arm(interpreter, axis=axis)
                decision = interpreter.process(packet(sequence=3, detents=1, axis=axis))
                self.assertEqual(decision.jog.delta_pulses, expected)

    def test_counterclockwise_is_the_exact_inverse_for_each_axis(self):
        for axis, expected in (("X", 10), ("Y", -10), ("Z", -10)):
            with self.subTest(axis=axis):
                interpreter = PendantInterpreter()
                self.arm(interpreter, axis=axis)
                decision = interpreter.process(packet(sequence=3, detents=-1, axis=axis))
                self.assertEqual(decision.jog.delta_pulses, expected)

    def test_large_absolute_count_jump_is_one_latest_signal_not_a_queue(self):
        interpreter = PendantInterpreter()
        self.arm(interpreter, axis="X", multiplier="X100")
        decision = interpreter.process(
            packet(
                sequence=3,
                detents=1_000_000,
                signal=1,
                axis="X",
                multiplier="X100",
            )
        )
        self.assertEqual(decision.jog.detent_delta, 1)
        self.assertEqual(decision.jog.delta_pulses, -1000)

    def test_released_deadman_discards_wheel_motion(self):
        interpreter = PendantInterpreter()
        interpreter.process(packet(sequence=1, deadman=False))
        decision = interpreter.process(packet(sequence=2, detents=9, deadman=False))
        self.assertTrue(decision.stop)
        decision = interpreter.process(packet(sequence=3, detents=9, deadman=True))
        self.assertTrue(decision.stop)
        decision = interpreter.process(packet(sequence=4, detents=10, deadman=True))
        self.assertEqual(decision.jog.delta_pulses, -10)

    def test_estop_and_unconfigured_axis_stop(self):
        interpreter = PendantInterpreter()
        self.arm(interpreter)
        self.assertTrue(interpreter.process(packet(sequence=3, estop=True)).stop)

        interpreter = PendantInterpreter()
        interpreter.process(packet(sequence=1, axis="4", deadman=False))
        self.assertTrue(interpreter.process(packet(sequence=2, axis="4")).stop)

    def test_quadrature_error_is_fatal(self):
        interpreter = PendantInterpreter()
        self.arm(interpreter)
        with self.assertRaises(ControlFault):
            interpreter.process(packet(sequence=3, errors=1))


class LimitTests(unittest.TestCase):
    def test_shared_machine_pulse_setting(self):
        self.assertEqual(MOTOR_PULSES_PER_REV, 4000)
        self.assertEqual(REFERENCE_PULSES_PER_REV, 800)
        self.assertEqual(PULSE_SCALE, 5)

    def test_user_selected_multiplier_jog_speeds(self):
        self.assertEqual(
            PREVIOUS_JOG_RATE_BY_MULTIPLIER,
            {"X1": 2500.0, "X10": 15000.0, "X100": 30000.0},
        )
        self.assertEqual(
            JOG_RATE_SCALE_BY_MULTIPLIER,
            {"X1": 2, "X10": 5, "X100": 10},
        )
        self.assertEqual(JOG_RATE_BY_MULTIPLIER["X1"], 5000.0)
        self.assertEqual(JOG_RATE_BY_MULTIPLIER["X10"], 75000.0)
        self.assertEqual(JOG_RATE_BY_MULTIPLIER["X100"], 300000.0)
        self.assertEqual(JOG_RATE_PULSES_PER_SECOND, 300000.0)

    def test_user_selected_bounce_preserves_physical_distance_and_speed(self):
        self.assertEqual(BOUNCE_PULSES, 250)
        self.assertEqual(BOUNCE_RATE_PULSES_PER_SECOND, 1500.0)

    def test_matching_toward_limit_event_requests_exact_scaled_bounce(self):
        action = decide_limit_action(
            active_motor=1,
            active_toward_limit=True,
            latched_by_motor=(False, True, False),
        )
        self.assertEqual(action.kind, "bounce")
        self.assertEqual(action.motor, 1)
        self.assertEqual(action.bounce_delta, -BOUNCE_PULSES)

        plan = make_bounce_plan(motor=1, stopped_count=12345)
        self.assertEqual(plan.stopped_count, 12345)
        self.assertEqual(plan.target_count, 12095)
        self.assertEqual(plan.delta_pulses, -250)

    def test_wrong_or_multiple_limit_is_fault(self):
        self.assertEqual(
            decide_limit_action(
                active_motor=1,
                active_toward_limit=True,
                latched_by_motor=(True, False, False),
            ).kind,
            "fault",
        )
        self.assertEqual(
            decide_limit_action(
                active_motor=1,
                active_toward_limit=True,
                latched_by_motor=(True, True, False),
            ).kind,
            "fault",
        )

    def test_limit_while_moving_away_is_fault_not_an_extra_move(self):
        action = decide_limit_action(
            active_motor=2,
            active_toward_limit=False,
            latched_by_motor=(False, False, True),
        )
        self.assertEqual(action.kind, "fault")


if __name__ == "__main__":
    unittest.main()
