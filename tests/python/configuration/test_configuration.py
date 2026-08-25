from __future__ import annotations

import contextlib
import io
import subprocess
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock

from tests.python._support import PROJECT_ROOT

from dmc2_runtime import launcher as launch_live
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

    def test_launcher_without_live_flag_cannot_exec_linuxcnc(self):
        output = io.StringIO()
        with mock.patch.object(
            launch_live,
            "validate_launch_files",
            return_value=["offline launcher contract"],
        ):
            with mock.patch.object(launch_live.os, "execvp") as execvp:
                with contextlib.redirect_stdout(output):
                    result = launch_live.main([])
        self.assertEqual(result, 0)
        execvp.assert_not_called()
        self.assertIn("VALIDATION ONLY", output.getvalue())

    def test_process_owner_probe_handles_every_documented_pgrep_result(self):
        for returncode, expected in ((0, True), (1, False)):
            completed = subprocess.CompletedProcess(
                args=["pgrep", "-f", "owner"],
                returncode=returncode,
            )
            with mock.patch.object(
                launch_live.subprocess,
                "run",
                return_value=completed,
            ):
                self.assertIs(launch_live.running_process("owner"), expected)

        for returncode in (2, 3, -9, 127):
            completed = subprocess.CompletedProcess(
                args=["pgrep", "-f", "owner"],
                returncode=returncode,
            )
            with mock.patch.object(
                launch_live.subprocess,
                "run",
                return_value=completed,
            ):
                with self.assertRaisesRegex(
                    RuntimeError,
                    f"pgrep exited {returncode}",
                ):
                    launch_live.running_process("owner")

    def test_launcher_refuses_when_exact_profile_validation_fails(self):
        with mock.patch.object(
            launch_live,
            "validate_offline_profile",
            side_effect=AssertionError("changed value"),
        ):
            with mock.patch.object(
                launch_live,
                "validate_realtime_module_deployment",
            ):
                with mock.patch.object(
                    launch_live,
                    "validate_userspace_binary_deployment",
                ):
                    with self.assertRaisesRegex(RuntimeError, "exact offline profile"):
                        launch_live.validate_launch_files()

    def test_launcher_refuses_any_linuxcnc_version_except_2_9_10(self):
        completed = subprocess.CompletedProcess(
            args=["linuxcnc_var", "LINUXCNCVERSION"],
            returncode=0,
            stdout="2.9.9\n",
            stderr="",
        )
        with mock.patch.object(launch_live.subprocess, "run", return_value=completed):
            with mock.patch.object(
                launch_live,
                "validate_realtime_module_deployment",
            ):
                with mock.patch.object(
                    launch_live,
                    "validate_userspace_binary_deployment",
                ):
                    with self.assertRaisesRegex(RuntimeError, "expected 2.9.10"):
                        launch_live.validate_launch_files()

    def test_launcher_refuses_mismatched_installed_realtime_module(self):
        with tempfile.TemporaryDirectory() as directory:
            staged = Path(directory) / "staged.so"
            installed = Path(directory) / "installed.so"
            staged.write_bytes(b"verified staged module")
            installed.write_bytes(b"different installed module")
            deployments = ((installed, staged),)
            with mock.patch.object(
                launch_live,
                "REALTIME_MODULE_DEPLOYMENTS",
                deployments,
            ):
                with self.assertRaisesRegex(RuntimeError, "does not match"):
                    launch_live.validate_realtime_module_deployment()

    def test_launcher_checks_both_realtime_modules_byte_for_byte(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            installed_dmc2 = root / "dmc2_rt.so"
            staged_dmc2 = root / "staged_dmc2_rt.so"
            installed_h100 = root / "h100_spindle.so"
            staged_h100 = root / "staged_h100_spindle.so"
            installed_dmc2.write_bytes(b"exact dmc2")
            staged_dmc2.write_bytes(b"exact dmc2")
            installed_h100.write_bytes(b"stale h100")
            staged_h100.write_bytes(b"exact h100")
            deployments = (
                (installed_dmc2, staged_dmc2),
                (installed_h100, staged_h100),
            )
            with mock.patch.object(
                launch_live,
                "REALTIME_MODULE_DEPLOYMENTS",
                deployments,
            ):
                with self.assertRaisesRegex(RuntimeError, "h100_spindle.so"):
                    launch_live.validate_realtime_module_deployment()

            installed_h100.write_bytes(b"exact h100")
            with mock.patch.object(
                launch_live,
                "REALTIME_MODULE_DEPLOYMENTS",
                deployments,
            ):
                launch_live.validate_realtime_module_deployment()

    def test_launcher_refuses_mismatched_userspace_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            deployed_serial = root / "deployed-serial"
            staged_serial = root / "staged-serial"
            deployed_monitor = root / "deployed-monitor"
            staged_monitor = root / "staged-monitor"
            deployed_serial.write_bytes(b"verified serial")
            staged_serial.write_bytes(b"verified serial")
            deployed_monitor.write_bytes(b"stale monitor")
            staged_monitor.write_bytes(b"verified monitor")
            deployments = (
                (deployed_serial, staged_serial),
                (deployed_monitor, staged_monitor),
            )
            with mock.patch.object(
                launch_live,
                "USERSPACE_BINARY_DEPLOYMENTS",
                deployments,
            ):
                with self.assertRaisesRegex(
                    RuntimeError,
                    "deployed-monitor does not match",
                ):
                    launch_live.validate_userspace_binary_deployment()

    def test_persistent_launcher_uses_detached_user_service(self):
        completed = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="Running as unit: dmc2-linuxcnc.service\n",
        )
        with mock.patch.object(
            launch_live.shutil,
            "which",
            return_value="/usr/bin/systemd-run",
        ):
            with mock.patch.object(
                launch_live.subprocess,
                "run",
                return_value=completed,
            ) as run:
                output = io.StringIO()
                with contextlib.redirect_stdout(output):
                    result = launch_live.start_persistent_service()

        self.assertEqual(result, 0)
        command = run.call_args.args[0]
        self.assertEqual(command[:3], ["systemd-run", "--user", "--unit=dmc2-linuxcnc"])
        self.assertIn("--setenv=LINUXCNC_FORCE_REALTIME=1", command)
        self.assertIn("--property=KillMode=control-group", command)
        self.assertIn("--property=Restart=no", command)
        self.assertEqual(command[-1], "--live")
        self.assertNotIn("--persistent", command)
        self.assertIn("PERSISTENT LIVE UNIT STARTED", output.getvalue())


if __name__ == "__main__":
    unittest.main()
