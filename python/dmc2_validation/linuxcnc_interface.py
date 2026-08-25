"""Behavioral validation of the compiled LinuxCNC interface boundary."""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path

from dmc2_axis.constants import ERROR_CHANNEL_KIND_DEFINITIONS

from .paths import PROJECT_ROOT as ROOT

EXPECTED_LINUXCNC_VERSION = "2.9.10"
EXPECTED_LINUXCNC_COMMIT = "86cdca76fa2a36274c432caa21952b23c267989a"
EXPECTED_AUDIT_VALUES = {
    "schema_version": 1,
    "linuxcnc_version": EXPECTED_LINUXCNC_VERSION,
    "linuxcnc_source_commit": EXPECTED_LINUXCNC_COMMIT,
    "catalog_domains": 91,
    "catalog_codes": 920,
    "enum_codes": 711,
    "non_enum_codes": 209,
    "public_enum_headers": 30,
    "enum_declarations": 79,
    "interpreter_error_templates": 198,
    "status_message_contracts": 12,
    "error_message_contracts": 6,
    "snapshot_abi_version": 0x00020911,
    "snapshot_size": 11672,
    "snapshot_logical_fields": 1109,
    "snapshot_field_bytes": 11165,
    "snapshot_padding_bytes": 507,
    "snapshot_copy_signature_rounds": 21,
    "snapshot_copy_all_bytes": True,
}
EXPECTED_AUDIT_KEYS = frozenset((*EXPECTED_AUDIT_VALUES, "snapshot_schema_fnv64"))


def _compiled_task_monitor() -> Path:
    binary = ROOT / "rust" / "target" / "release" / "dmc2-task-monitor"
    if binary.is_file():
        return binary
    raise AssertionError(
        "release dmc2-task-monitor is missing; run the verified release build before validation"
    )


def compiled_linuxcnc_audit() -> dict[str, object]:
    """Execute the standard binary's native, source-derived audit path."""
    binary = _compiled_task_monitor()
    result = subprocess.run(
        [str(binary), "--validate-json"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise AssertionError(
            "compiled LinuxCNC interface audit failed: "
            f"exit={result.returncode} stderr={result.stderr.strip()!r}"
        )
    output_lines = [line for line in result.stdout.splitlines() if line.strip()]
    if len(output_lines) != 1:
        raise AssertionError(
            "compiled LinuxCNC interface audit must emit exactly one JSON record, "
            f"found {output_lines!r}"
        )
    try:
        report = json.loads(output_lines[0])
    except json.JSONDecodeError as error:
        raise AssertionError(
            f"compiled LinuxCNC interface audit emitted invalid JSON: {output_lines[0]!r}"
        ) from error
    if not isinstance(report, dict):
        raise AssertionError("compiled LinuxCNC interface audit did not emit an object")
    if set(report) != EXPECTED_AUDIT_KEYS:
        raise AssertionError(
            "compiled LinuxCNC interface audit schema changed: "
            f"missing={sorted(EXPECTED_AUDIT_KEYS - set(report))} "
            f"extra={sorted(set(report) - EXPECTED_AUDIT_KEYS)}"
        )
    mismatches = {
        name: (expected, report.get(name))
        for name, expected in EXPECTED_AUDIT_VALUES.items()
        if report.get(name) != expected
    }
    if mismatches:
        raise AssertionError(
            f"compiled LinuxCNC interface audit values changed: {mismatches}"
        )
    schema_fingerprint = report["snapshot_schema_fnv64"]
    if not isinstance(schema_fingerprint, str) or not re.fullmatch(
        r"0x[0-9a-f]{16}", schema_fingerprint
    ):
        raise AssertionError(
            f"invalid snapshot schema fingerprint: {schema_fingerprint!r}"
        )
    if int(schema_fingerprint, 16) == 0:
        raise AssertionError("snapshot schema fingerprint must not be zero")
    if report["enum_codes"] + report["non_enum_codes"] != report["catalog_codes"]:
        raise AssertionError("compiled code-domain totals do not add up")
    if (
        report["snapshot_field_bytes"] + report["snapshot_padding_bytes"]
        != report["snapshot_size"]
    ):
        raise AssertionError("compiled snapshot byte totals do not add up")
    return report


def _validate_current_linuxcnc_checkout() -> None:
    source_root = ROOT / "vendor" / "linuxcnc-2.9.10"
    if not source_root.is_dir():
        raise AssertionError("durable LinuxCNC 2.9.10 source clone is missing")
    installed_version = subprocess.run(
        ["linuxcnc_var", "LINUXCNCVERSION"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if installed_version != EXPECTED_LINUXCNC_VERSION:
        raise AssertionError(
            f"installed LinuxCNC must be {EXPECTED_LINUXCNC_VERSION}, "
            f"found {installed_version}"
        )
    source_commit = subprocess.run(
        ["git", "-C", str(source_root), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if source_commit != EXPECTED_LINUXCNC_COMMIT:
        raise AssertionError(
            f"LinuxCNC source must be v2.9.10 commit {EXPECTED_LINUXCNC_COMMIT}, "
            f"found {source_commit}"
        )
    source_changes = subprocess.run(
        ["git", "-C", str(source_root), "status", "--porcelain"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if source_changes:
        raise AssertionError("LinuxCNC 2.9.10 source clone has local modifications")


def _validate_axis_error_channel_contract() -> None:
    expected = (
        ("NML_ERROR", 1, "error"),
        ("NML_TEXT", 2, "info"),
        ("NML_DISPLAY", 3, "info"),
        ("OPERATOR_ERROR", 11, "error"),
        ("OPERATOR_TEXT", 12, "info"),
        ("OPERATOR_DISPLAY", 13, "info"),
    )
    if ERROR_CHANNEL_KIND_DEFINITIONS != expected:
        raise AssertionError(
            "AXIS error-channel definitions differ from the six compiled LinuxCNC layouts"
        )


def validate_linuxcnc_interface_coverage() -> str:
    _validate_current_linuxcnc_checkout()
    report = compiled_linuxcnc_audit()
    _validate_axis_error_channel_contract()
    return (
        f"compiled LinuxCNC {report['linuxcnc_version']} audit covers "
        f"{report['catalog_domains']} domains / {report['catalog_codes']} codes, "
        f"all {report['enum_declarations']} public enum declarations, "
        f"{report['interpreter_error_templates']} interpreter errors, all six "
        f"error-channel layouts, and every byte of the {report['snapshot_size']}-byte "
        f"status snapshot across {report['snapshot_copy_signature_rounds']} signature rounds"
    )
