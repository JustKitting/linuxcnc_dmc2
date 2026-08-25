from __future__ import annotations

import unittest

from tests.python import _support  # noqa: F401

from dmc2_reference.watchdog import (
    HEARTBEAT_TOGGLE_SECONDS,
    PHASE_ARM_LOW,
    PHASE_FAULT,
    PHASE_READY,
    PHASE_STABLE,
    PHASE_WAIT_PREREQUISITES,
    PHASE_WAIT_OK,
    ControllerWatchdogStartupGuard,
    HeartbeatGenerator,
)


class HeartbeatGeneratorTests(unittest.TestCase):
    def test_level_persists_across_many_one_kilohertz_servo_samples(self):
        heartbeat = HeartbeatGenerator()
        samples = [heartbeat.update(index / 1000.0) for index in range(101)]
        transitions = sum(
            current != previous
            for previous, current in zip(samples, samples[1:])
        )

        self.assertEqual(HEARTBEAT_TOGGLE_SECONDS, 0.020)
        self.assertEqual(transitions, 5)
        self.assertTrue(all(not value for value in samples[:20]))
        self.assertTrue(all(samples[index] for index in range(20, 40)))

    def test_late_update_toggles_once_and_rebases_without_catch_up_burst(self):
        heartbeat = HeartbeatGenerator()
        self.assertFalse(heartbeat.update(0.000))
        self.assertTrue(heartbeat.update(0.095))
        self.assertTrue(heartbeat.update(0.096))
        self.assertFalse(heartbeat.update(0.115))


class ControllerWatchdogStartupGuardTests(unittest.TestCase):
    def arm_and_finish(self, guard: ControllerWatchdogStartupGuard) -> None:
        guard.update(
            now=0.000,
            watchdog_ok=False,
            prerequisites_ready=True,
        )
        self.assertEqual(guard.phase, PHASE_ARM_LOW)
        self.assertFalse(guard.enable)

        guard.update(
            now=0.011,
            watchdog_ok=False,
            prerequisites_ready=True,
        )
        self.assertEqual(guard.phase, PHASE_WAIT_OK)
        self.assertTrue(guard.enable)

        guard.update(
            now=0.012,
            watchdog_ok=True,
            prerequisites_ready=True,
        )
        self.assertEqual(guard.phase, PHASE_STABLE)
        guard.update(
            now=0.263,
            watchdog_ok=True,
            prerequisites_ready=True,
        )
        self.assertEqual(guard.phase, PHASE_READY)
        self.assertTrue(guard.ready)
        self.assertTrue(guard.enable)

    def test_clean_arm_requires_low_phase_and_stable_ok_feedback(self):
        guard = ControllerWatchdogStartupGuard()
        self.arm_and_finish(guard)
        self.assertEqual(guard.arm_attempts, 1)
        self.assertFalse(guard.faulted)

    def test_waits_fail_closed_for_post_gui_and_live_input_prerequisites(self):
        guard = ControllerWatchdogStartupGuard()
        guard.update(
            now=0.000,
            watchdog_ok=False,
            prerequisites_ready=False,
        )
        guard.update(
            now=300.000,
            watchdog_ok=False,
            prerequisites_ready=False,
        )

        self.assertEqual(guard.phase, PHASE_WAIT_PREREQUISITES)
        self.assertFalse(guard.enable)
        self.assertFalse(guard.ready)

        guard.update(
            now=300.001,
            watchdog_ok=False,
            prerequisites_ready=True,
        )
        self.assertEqual(guard.phase, PHASE_ARM_LOW)
        self.assertFalse(guard.enable)

    def test_recorded_192_millisecond_startup_gap_is_rearmed_not_misreported(self):
        guard = ControllerWatchdogStartupGuard()
        guard.update(
            now=0.000,
            watchdog_ok=False,
            prerequisites_ready=True,
        )
        guard.update(
            now=0.011,
            watchdog_ok=False,
            prerequisites_ready=True,
        )
        self.assertTrue(guard.enable)

        # The live failure paused userspace for 192 ms. The 100 ms realtime
        # watchdog correctly timed out; startup must drive enable low before
        # making a second clean arm attempt.
        guard.update(
            now=0.203,
            watchdog_ok=False,
            prerequisites_ready=True,
        )
        self.assertEqual(guard.phase, PHASE_ARM_LOW)
        self.assertFalse(guard.enable)

        guard.update(
            now=0.214,
            watchdog_ok=False,
            prerequisites_ready=True,
        )
        self.assertEqual(guard.phase, PHASE_WAIT_OK)
        self.assertTrue(guard.enable)
        guard.update(
            now=0.215,
            watchdog_ok=True,
            prerequisites_ready=True,
        )
        guard.update(
            now=0.466,
            watchdog_ok=True,
            prerequisites_ready=True,
        )

        self.assertTrue(guard.ready)
        self.assertEqual(guard.arm_attempts, 2)
        self.assertFalse(guard.faulted)

    def test_recorded_post_gui_107_millisecond_gap_remains_startup_rearmable(self):
        guard = ControllerWatchdogStartupGuard()
        guard.update(
            now=0.000,
            watchdog_ok=False,
            prerequisites_ready=False,
        )
        guard.update(
            now=1.223,
            watchdog_ok=False,
            prerequisites_ready=True,
        )
        guard.update(
            now=1.234,
            watchdog_ok=False,
            prerequisites_ready=True,
        )
        guard.update(
            now=1.235,
            watchdog_ok=True,
            prerequisites_ready=True,
        )

        # This replays the observed event only; it is evidence, not a startup
        # duration bound. The unbounded-wait tests below cover arbitrary time.
        guard.update(
            now=1.396,
            watchdog_ok=False,
            prerequisites_ready=True,
        )
        self.assertEqual(guard.phase, PHASE_ARM_LOW)
        self.assertFalse(guard.enable)

        guard.update(
            now=1.407,
            watchdog_ok=False,
            prerequisites_ready=True,
        )
        guard.update(
            now=1.408,
            watchdog_ok=True,
            prerequisites_ready=True,
        )
        guard.update(
            now=1.659,
            watchdog_ok=True,
            prerequisites_ready=True,
        )

        self.assertEqual(guard.phase, PHASE_READY)
        self.assertTrue(guard.ready)
        self.assertEqual(guard.arm_attempts, 2)
        self.assertFalse(guard.faulted)

    def test_watchdog_that_never_becomes_ok_waits_fail_closed_without_timeout(self):
        guard = ControllerWatchdogStartupGuard()
        guard.update(
            now=0.000,
            watchdog_ok=False,
            prerequisites_ready=True,
        )
        guard.update(
            now=0.011,
            watchdog_ok=False,
            prerequisites_ready=True,
        )
        guard.update(
            now=300.011,
            watchdog_ok=False,
            prerequisites_ready=True,
        )

        self.assertEqual(guard.phase, PHASE_ARM_LOW)
        self.assertFalse(guard.faulted)
        self.assertFalse(guard.enable)

    def test_arbitrary_pause_before_runtime_commit_rearms_without_fault(self):
        guard = ControllerWatchdogStartupGuard()
        self.arm_and_finish(guard)

        guard.update(
            now=300.263,
            watchdog_ok=False,
            prerequisites_ready=True,
        )

        self.assertEqual(guard.phase, PHASE_ARM_LOW)
        self.assertFalse(guard.ready)
        self.assertFalse(guard.enable)
        self.assertFalse(guard.faulted)

    def test_prerequisite_loss_before_runtime_commit_returns_to_unbounded_wait(self):
        guard = ControllerWatchdogStartupGuard()
        self.arm_and_finish(guard)

        guard.update(
            now=300.263,
            watchdog_ok=True,
            prerequisites_ready=False,
        )
        guard.update(
            now=600.263,
            watchdog_ok=False,
            prerequisites_ready=False,
        )

        self.assertEqual(guard.phase, PHASE_WAIT_PREREQUISITES)
        self.assertFalse(guard.ready)
        self.assertFalse(guard.enable)
        self.assertFalse(guard.faulted)

    def test_runtime_watchdog_drop_is_not_automatically_rearmed(self):
        guard = ControllerWatchdogStartupGuard()
        self.arm_and_finish(guard)
        guard.commit_runtime()
        self.assertTrue(guard.runtime_committed)

        guard.update(
            now=0.300,
            watchdog_ok=False,
            prerequisites_ready=True,
        )

        self.assertEqual(guard.phase, PHASE_FAULT)
        self.assertTrue(guard.faulted)
        self.assertFalse(guard.enable)
        self.assertIn("after startup", guard.fault or "")


if __name__ == "__main__":
    unittest.main()
