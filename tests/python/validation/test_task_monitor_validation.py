from __future__ import annotations

import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from dmc2_validation import task_monitor


class CompiledTaskMonitorValidationTests(unittest.TestCase):
    @staticmethod
    def valid_report() -> dict[str, object]:
        return {
            **task_monitor.EXPECTED_VALIDATION_VALUES,
            "snapshot_schema_fnv64": "0x7a1f54f8088b6ed9",
        }

    def run_with(self, stdout: str, *, returncode: int = 0, stderr: str = ""):
        completed = subprocess.CompletedProcess(
            args=["dmc2-task-monitor", "--validate-json"],
            returncode=returncode,
            stdout=stdout,
            stderr=stderr,
        )
        with mock.patch.object(
            task_monitor,
            "_compiled_task_monitor",
            return_value=Path("/compiled/dmc2-task-monitor"),
        ):
            with mock.patch.object(
                task_monitor.subprocess,
                "run",
                return_value=completed,
            ):
                return task_monitor.compiled_task_monitor_validation()

    def test_exact_compiled_report_is_accepted(self):
        report = self.valid_report()
        self.assertEqual(self.run_with(json.dumps(report)), report)

    def test_only_the_release_binary_is_accepted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            debug = root / "rust" / "target" / "debug" / "dmc2-task-monitor"
            debug.parent.mkdir(parents=True)
            debug.write_bytes(b"debug")
            with mock.patch.object(task_monitor, "ROOT", root):
                with self.assertRaisesRegex(AssertionError, "release dmc2-task-monitor"):
                    task_monitor._compiled_task_monitor()

                release = root / "rust" / "target" / "release" / "dmc2-task-monitor"
                release.parent.mkdir(parents=True)
                release.write_bytes(b"release")
                self.assertEqual(task_monitor._compiled_task_monitor(), release)

    def test_nonzero_binary_exit_is_rejected(self):
        with self.assertRaisesRegex(AssertionError, "exit=7"):
            self.run_with("", returncode=7, stderr="native copy failed")

    def test_invalid_json_is_rejected(self):
        with self.assertRaisesRegex(AssertionError, "invalid JSON"):
            self.run_with("not-json")

    def test_multiple_output_records_are_rejected(self):
        record = json.dumps(self.valid_report())
        with self.assertRaisesRegex(AssertionError, "exactly one JSON record"):
            self.run_with(f"{record}\n{record}\n")

    def test_missing_or_extra_schema_keys_are_rejected(self):
        report = self.valid_report()
        del report["snapshot_size"]
        report["invented"] = 1
        with self.assertRaisesRegex(AssertionError, "schema changed"):
            self.run_with(json.dumps(report))

    def test_changed_contract_value_is_rejected(self):
        report = self.valid_report()
        report["snapshot_size"] = 1
        with self.assertRaisesRegex(AssertionError, "validation values changed"):
            self.run_with(json.dumps(report))

    def test_inconsistent_field_ownership_is_rejected(self):
        report = self.valid_report()
        report["snapshot_logical_fields"] = 1_108
        with self.assertRaisesRegex(AssertionError, "field ownership totals"):
            self.run_with(json.dumps(report))

    def test_invalid_or_zero_schema_fingerprint_is_rejected(self):
        report = self.valid_report()
        report["snapshot_schema_fnv64"] = "0x0000000000000000"
        with self.assertRaisesRegex(AssertionError, "must not be zero"):
            self.run_with(json.dumps(report))
        report["snapshot_schema_fnv64"] = "not-a-fingerprint"
        with self.assertRaisesRegex(AssertionError, "invalid task-monitor snapshot"):
            self.run_with(json.dumps(report))


if __name__ == "__main__":
    unittest.main()
