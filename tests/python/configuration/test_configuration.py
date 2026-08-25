from __future__ import annotations

import types
import unittest

from tests.python._support import PROJECT_ROOT

from dmc2_validation import (
    ROOT,
    READY_CONFIGURATION_STATUS,
    load_requirements,
    read_ini,
    unresolved_requirements,
    validate,
)
from dmc2_reference.controller import (
    LinuxCncBackend,
    coherent_nano_snapshot_from_component,
    publish_position_validity,
)


class ConfigurationTests(unittest.TestCase):
    @staticmethod
    def nano_component(generation=2):
        return {
            "snapshot-generation": generation,
            "connected": True,
            "serial-fault": False,
            "quadrature-fault": False,
            "axis-code": 0,
            "multiplier-code": 1,
            "latest-detent": 1,
            "sequence": 9,
            "milliseconds": 180,
            "detent-count": 1,
            "transition-count": 4,
            "quadrature-errors": 0,
            "deadman-held": True,
            "estop-pressed": False,
            "selector-valid": True,
        }

    def test_coherent_nano_snapshot_accepts_one_stable_even_publication(self):
        snapshot = coherent_nano_snapshot_from_component(self.nano_component())
        self.assertIsNotNone(snapshot)
        self.assertTrue(snapshot.connected)
        self.assertEqual(snapshot.packet.sequence, 9)
        self.assertEqual(snapshot.packet.latest_detent_signal, 1)

    def test_coherent_nano_snapshot_rejects_a_busy_publication(self):
        self.assertIsNone(
            coherent_nano_snapshot_from_component(
                self.nano_component(generation=3),
                attempts=3,
            )
        )

    def test_coherent_nano_snapshot_rejects_generation_change_mid_read(self):
        class ChangingGeneration(dict):
            def __init__(self, values):
                super().__init__(values)
                self.generation_reads = 0

            def __getitem__(self, key):
                if key == "snapshot-generation":
                    self.generation_reads += 1
                    return 2 if self.generation_reads == 1 else 4
                return super().__getitem__(key)

        self.assertIsNone(
            coherent_nano_snapshot_from_component(
                ChangingGeneration(self.nano_component()),
                attempts=1,
            )
        )

    def test_position_is_unknown_until_all_three_joints_are_homed(self):
        component = {}
        publish_position_validity(
            component,
            types.SimpleNamespace(all_homed=False),
        )
        self.assertFalse(component["position-known"])
        self.assertTrue(component["position-unknown"])

        publish_position_validity(
            component,
            types.SimpleNamespace(all_homed=True),
        )
        self.assertTrue(component["position-known"])
        self.assertFalse(component["position-unknown"])

    def test_recovery_state_requests_are_delegated_to_halui(self):
        class RecordingCommand:
            def __init__(self):
                self.states = []

            def state(self, requested_state):
                self.states.append(requested_state)

            def wait_complete(self, _timeout):
                raise AssertionError("recovery command blocked the heartbeat loop")

        backend = object.__new__(LinuxCncBackend)
        backend.command = RecordingCommand()
        backend.component = {
            "estop-reset-request": False,
            "machine-on-request": False,
        }

        backend.request_estop_reset()
        self.assertTrue(backend.component["estop-reset-request"])
        self.assertFalse(backend.component["machine-on-request"])

        backend.request_machine_on()
        self.assertFalse(backend.component["estop-reset-request"])
        self.assertTrue(backend.component["machine-on-request"])

        backend.clear_state_requests()
        self.assertFalse(backend.component["estop-reset-request"])
        self.assertFalse(backend.component["machine-on-request"])

        self.assertEqual(backend.command.states, [])

    def test_jog_is_flushed_only_after_realtime_gate_can_be_published(self):
        class RecordingCommand:
            def __init__(self):
                self.jogs = []

            def jog(self, *arguments):
                self.jogs.append(arguments)

        backend = object.__new__(LinuxCncBackend)
        backend.linuxcnc = types.SimpleNamespace(JOG_INCREMENT="increment")
        backend.command = RecordingCommand()
        backend._pending_jogs = []

        backend.jog_increment(
            0,
            -1.5,
            0.25,
            joint_jog=True,
        )

        self.assertEqual(backend.command.jogs, [])
        backend.flush_pending_jogs()
        self.assertEqual(
            backend.command.jogs,
            [("increment", True, 0, -1.5, 0.25)],
        )
        self.assertEqual(backend._pending_jogs, [])

    def test_static_offline_validation(self):
        checks = validate()
        self.assertGreaterEqual(len(checks), 10)

    def test_accepted_profile_has_no_blocking_requirement(self):
        data = load_requirements(ROOT / "live_requirements.json")
        self.assertEqual(data["configuration_status"], READY_CONFIGURATION_STATUS)
        self.assertEqual(unresolved_requirements(data), [])
        deferred_ids = {
            item["id"]
            for item in data["requirements"]
            if not item.get("blocking", True)
        }
        self.assertIn("probe_contact_motion_behavior", deferred_ids)
        self.assertIn("motor_alarm_input_mapping_and_polarity", deferred_ids)
        self.assertIn("physical_drive_enable_output_policy", deferred_ids)
        self.assertIn("spindle_hardware_control", deferred_ids)

    def test_exact_accepted_safe_zone_and_home_values(self):
        config = read_ini(ROOT / "live" / "dmc2.ini")
        expected = (
            ("X", "JOINT_0", 300.0, 300.25, "0"),
            ("Y", "JOINT_1", 173.0, 173.25, "1"),
            ("Z", "JOINT_2", 135.0, 135.25, "2"),
        )
        for axis, joint, home, switch, sequence in expected:
            self.assertEqual(config.getfloat(f"AXIS_{axis}", "MIN_LIMIT"), 0.0)
            self.assertEqual(config.getfloat(f"AXIS_{axis}", "MAX_LIMIT"), home)
            self.assertEqual(config.getfloat(joint, "SCALE"), 1000.0)
            self.assertEqual(config.getfloat(joint, "HOME"), home)
            self.assertEqual(config.getfloat(joint, "HOME_OFFSET"), switch)
            self.assertEqual(config.getfloat(joint, "HOME_SEARCH_VEL"), 5.0)
            self.assertEqual(config.getfloat(joint, "HOME_LATCH_VEL"), 0.25)
            self.assertEqual(config.getfloat(joint, "HOME_FINAL_VEL"), 0.25)
            self.assertEqual(config.getfloat(joint, "MAX_ACCELERATION"), 50.0)
            self.assertEqual(config.getfloat(joint, "FERROR"), 0.050)
            self.assertEqual(config.getfloat(joint, "MIN_FERROR"), 0.010)
            self.assertEqual(config.getfloat(joint, "STEPGEN_MAX_ACC"), 0.0)
            self.assertEqual(config.get(joint, "HOME_SEQUENCE"), sequence)

if __name__ == "__main__":
    unittest.main()
