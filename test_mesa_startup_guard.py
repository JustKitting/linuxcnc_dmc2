from __future__ import annotations

import unittest

from mesa_startup_guard import (
    PHASE_FAULT,
    PHASE_LIMIT_RESET,
    PHASE_LIMIT_SETTLE,
    PHASE_READY,
    PHASE_WAIT_SERVO,
    PHASE_WATCHDOG_STABLE,
    MesaStartupGuard,
)


class MesaStartupGuardTests(unittest.TestCase):
    def advance_to_ready(self, guard: MesaStartupGuard) -> None:
        guard.update(now=0.000, servo_thread_ready=True, watchdog_has_bit=True)
        self.assertTrue(guard.watchdog_clear_requested)
        guard.update(now=0.001, servo_thread_ready=True, watchdog_has_bit=False)
        guard.update(now=0.102, servo_thread_ready=True, watchdog_has_bit=False)
        self.assertEqual(guard.phase, PHASE_LIMIT_RESET)
        guard.update(now=0.113, servo_thread_ready=True, watchdog_has_bit=False)
        self.assertEqual(guard.phase, PHASE_LIMIT_SETTLE)
        guard.update(now=0.124, servo_thread_ready=True, watchdog_has_bit=False)
        self.assertEqual(guard.phase, PHASE_READY)
        self.assertTrue(guard.ready)

    def test_stale_bite_is_cleared_before_limit_latches_are_reset(self):
        guard = MesaStartupGuard()

        guard.update(now=0.000, servo_thread_ready=False, watchdog_has_bit=True)
        self.assertEqual(guard.phase, PHASE_WAIT_SERVO)
        self.assertFalse(guard.watchdog_clear_requested)
        self.assertEqual(guard.limit_reset, (False, False, False))

        guard.update(now=0.100, servo_thread_ready=True, watchdog_has_bit=True)
        self.assertEqual(guard.phase, PHASE_WATCHDOG_STABLE)
        self.assertTrue(guard.watchdog_clear_requested)

        guard.update(now=0.101, servo_thread_ready=True, watchdog_has_bit=False)
        guard.update(now=0.202, servo_thread_ready=True, watchdog_has_bit=False)
        self.assertEqual(guard.phase, PHASE_LIMIT_RESET)
        self.assertEqual(guard.limit_reset, (True, True, True))

        guard.update(now=0.213, servo_thread_ready=True, watchdog_has_bit=False)
        self.assertEqual(guard.phase, PHASE_LIMIT_SETTLE)
        self.assertEqual(guard.limit_reset, (False, False, False))

        guard.update(now=0.224, servo_thread_ready=True, watchdog_has_bit=False)
        self.assertTrue(guard.ready)
        self.assertFalse(guard.faulted)

    def test_reasserted_bite_restarts_startup_recovery(self):
        guard = MesaStartupGuard()
        guard.update(now=0.000, servo_thread_ready=True, watchdog_has_bit=False)
        guard.update(now=0.050, servo_thread_ready=True, watchdog_has_bit=True)

        self.assertFalse(guard.faulted)
        self.assertFalse(guard.ready)
        self.assertTrue(guard.watchdog_clear_requested)
        self.assertEqual(guard.phase, PHASE_WATCHDOG_STABLE)
        self.assertEqual(guard.limit_reset, (False, False, False))

    def test_runtime_bite_faults_and_is_not_automatically_cleared(self):
        guard = MesaStartupGuard()
        self.advance_to_ready(guard)

        guard.update(now=0.200, servo_thread_ready=True, watchdog_has_bit=True)

        self.assertEqual(guard.phase, PHASE_FAULT)
        self.assertTrue(guard.faulted)
        self.assertFalse(guard.watchdog_clear_requested)
        self.assertIn("after startup", guard.fault or "")

    def test_missing_servo_thread_waits_fail_closed_without_startup_timeout(self):
        guard = MesaStartupGuard()
        guard.update(now=0.000, servo_thread_ready=False, watchdog_has_bit=True)
        guard.update(now=300.000, servo_thread_ready=False, watchdog_has_bit=True)

        self.assertFalse(guard.faulted)
        self.assertFalse(guard.watchdog_clear_requested)
        self.assertEqual(guard.limit_reset, (False, False, False))
        self.assertEqual(guard.phase, PHASE_WAIT_SERVO)

    def test_uncleared_startup_watchdog_retries_without_elapsed_time_fault(self):
        guard = MesaStartupGuard()
        guard.update(now=0.000, servo_thread_ready=True, watchdog_has_bit=True)
        guard.update(now=300.000, servo_thread_ready=True, watchdog_has_bit=True)

        self.assertFalse(guard.faulted)
        self.assertFalse(guard.ready)
        self.assertTrue(guard.watchdog_clear_requested)
        self.assertEqual(guard.phase, PHASE_WATCHDOG_STABLE)

    def test_low_level_io_error_faults_without_attempting_a_reset(self):
        guard = MesaStartupGuard()
        guard.update(
            now=0.000,
            servo_thread_ready=True,
            watchdog_has_bit=True,
            io_error=True,
        )

        self.assertTrue(guard.faulted)
        self.assertFalse(guard.watchdog_clear_requested)
        self.assertIn("I/O error during startup", guard.fault or "")


if __name__ == "__main__":
    unittest.main()
