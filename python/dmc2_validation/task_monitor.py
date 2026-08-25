"""Validation of the compiled task-monitor program boundary."""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path
from types import MappingProxyType

from .paths import PROJECT_ROOT as ROOT

EXPECTED_VALIDATION_VALUES = MappingProxyType({
    "schema_version": 2,
    "linuxcnc_version": "2.9.10",
    "linuxcnc_source_commit": "86cdca76fa2a36274c432caa21952b23c267989a",
    "interface_domains": 91,
    "interface_codes": 920,
    "interface_handled_codes": 920,
    "interface_enum_codes": 711,
    "interface_non_enum_codes": 209,
    "interface_enum_headers": 30,
    "interface_enum_declarations": 79,
    "interpreter_errors": 198,
    "handled_interpreter_errors": 198,
    "status_message_contracts": 12,
    "error_message_contracts": 6,
    "snapshot_abi_version": 0x00020911,
    "snapshot_size": 11672,
    "snapshot_logical_fields": 1109,
    "snapshot_native_copy_fields": 1100,
    "snapshot_rust_derived_fields": 9,
    "snapshot_field_bytes": 11165,
    "snapshot_padding_bytes": 507,
    "snapshot_copy_signature_rounds": 21,
    "interface_all_codes_accounted": True,
    "snapshot_copy_all_bytes": True,
})
EXPECTED_VALIDATION_KEYS = frozenset(
    (*EXPECTED_VALIDATION_VALUES, "snapshot_schema_fnv64")
)


def _compiled_task_monitor() -> Path:
    binary = ROOT / "rust" / "target" / "release" / "dmc2-task-monitor"
    if binary.is_file():
        return binary
    raise AssertionError(
        "release dmc2-task-monitor is missing; run the verified release build before validation"
    )


def compiled_task_monitor_validation() -> dict[str, object]:
    """Execute the task monitor's compiled status-copy self-test."""
    binary = _compiled_task_monitor()
    result = subprocess.run(
        [str(binary), "--validate-json"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise AssertionError(
            "compiled task-monitor validation failed: "
            f"exit={result.returncode} stderr={result.stderr.strip()!r}"
        )
    output_lines = [line for line in result.stdout.splitlines() if line.strip()]
    if len(output_lines) != 1:
        raise AssertionError(
            "compiled task-monitor validation must emit exactly one JSON record, "
            f"found {output_lines!r}"
        )
    try:
        report = json.loads(output_lines[0])
    except json.JSONDecodeError as error:
        raise AssertionError(
            f"compiled task-monitor validation emitted invalid JSON: {output_lines[0]!r}"
        ) from error
    if not isinstance(report, dict):
        raise AssertionError("compiled task-monitor validation did not emit an object")
    if set(report) != EXPECTED_VALIDATION_KEYS:
        raise AssertionError(
            "compiled task-monitor validation schema changed: "
            f"missing={sorted(EXPECTED_VALIDATION_KEYS - set(report))} "
            f"extra={sorted(set(report) - EXPECTED_VALIDATION_KEYS)}"
        )
    if (
        report["interface_enum_codes"] + report["interface_non_enum_codes"]
        != report["interface_codes"]
    ):
        raise AssertionError("task-monitor interface code totals do not add up")
    if report["interface_handled_codes"] != report["interface_codes"]:
        raise AssertionError("task-monitor omitted a LinuxCNC interface code")
    if report["handled_interpreter_errors"] != report["interpreter_errors"]:
        raise AssertionError("task-monitor omitted a LinuxCNC interpreter error")
    if (
        report["snapshot_native_copy_fields"]
        + report["snapshot_rust_derived_fields"]
        != report["snapshot_logical_fields"]
    ):
        raise AssertionError("task-monitor field ownership totals do not add up")
    mismatches = {
        name: (expected, report.get(name))
        for name, expected in EXPECTED_VALIDATION_VALUES.items()
        if report.get(name) != expected
    }
    if mismatches:
        raise AssertionError(
            f"compiled task-monitor validation values changed: {mismatches}"
        )
    schema_fingerprint = report["snapshot_schema_fnv64"]
    if not isinstance(schema_fingerprint, str) or not re.fullmatch(
        r"0x[0-9a-f]{16}", schema_fingerprint
    ):
        raise AssertionError(
            f"invalid task-monitor snapshot fingerprint: {schema_fingerprint!r}"
        )
    if int(schema_fingerprint, 16) == 0:
        raise AssertionError("task-monitor snapshot fingerprint must not be zero")
    if (
        report["snapshot_field_bytes"] + report["snapshot_padding_bytes"]
        != report["snapshot_size"]
    ):
        raise AssertionError("task-monitor snapshot byte totals do not add up")
    return report


def validate_task_monitor_contract() -> str:
    report = compiled_task_monitor_validation()
    return (
        "compiled task monitor dispatches all "
        f"{report['interface_codes']} LinuxCNC codes and "
        f"{report['interpreter_errors']} interpreter errors, and accounts for every byte of its "
        f"{report['snapshot_size']}-byte status snapshot across "
        f"{report['snapshot_copy_signature_rounds']} signature rounds and derives "
        f"{report['snapshot_rust_derived_fields']} runtime fields in Rust"
    )
