"""Validation of the compiled task-monitor program boundary."""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path
from types import MappingProxyType

from .paths import PROJECT_ROOT as ROOT

EXPECTED_VALIDATION_VALUES = MappingProxyType({
    "schema_version": 4,
    "linuxcnc_version": "2.9.10",
    "linuxcnc_source_commit": "86cdca76fa2a36274c432caa21952b23c267989a",
    "interface_domains": 91,
    "interface_codes": 920,
    "interface_handled_codes": 920,
    "interface_enum_codes": 711,
    "interface_non_enum_codes": 209,
    "interface_enum_headers": 30,
    "interface_enum_declarations": 79,
    "public_headers": 120,
    "public_header_source_bytes": 635278,
    "public_macro_declarations": 1106,
    "public_macros": 1029,
    "public_macro_inactive": 86,
    "public_macro_function_like": 120,
    "public_macro_object_without_value": 126,
    "public_macro_signed_integer": 166,
    "public_macro_unsigned_integer": 315,
    "public_macro_not_integer_constant": 216,
    "public_integer_macros": 481,
    "handled_public_integer_macros": 481,
    "interpreter_errors": 198,
    "handled_interpreter_errors": 198,
    "status_message_contracts": 12,
    "error_message_contracts": 6,
    "error_message_object_bytes": 1656,
    "error_message_field_bytes": 1629,
    "error_message_padding_bytes": 27,
    "snapshot_abi_version": 0x00020911,
    "snapshot_size": 11672,
    "snapshot_logical_fields": 1109,
    "snapshot_native_copy_fields": 1100,
    "snapshot_rust_derived_fields": 9,
    "snapshot_field_bytes": 11165,
    "snapshot_padding_bytes": 507,
    "snapshot_copy_signature_rounds": 21,
    "interface_all_codes_accounted": True,
    "interface_all_public_macros_classified": True,
    "error_message_all_bytes_accounted": True,
    "snapshot_copy_all_bytes": True,
})
EXPECTED_VALIDATION_KEYS = frozenset(
    (
        *EXPECTED_VALIDATION_VALUES,
        "public_header_source_fnv64",
        "snapshot_schema_fnv64",
    )
)
EXPECTED_PUBLIC_HEADER_SOURCE_FNV64 = "0x8f2986fcf6b52329"
EXPECTED_SNAPSHOT_SCHEMA_FNV64 = "0x7eb51e89a20d7605"


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
    macro_kind_total = sum(
        report[name]
        for name in (
            "public_macro_inactive",
            "public_macro_function_like",
            "public_macro_object_without_value",
            "public_macro_signed_integer",
            "public_macro_unsigned_integer",
            "public_macro_not_integer_constant",
        )
    )
    if macro_kind_total != report["public_macros"]:
        raise AssertionError("task-monitor public macro classification totals do not add up")
    if (
        report["public_macro_signed_integer"]
        + report["public_macro_unsigned_integer"]
        != report["public_integer_macros"]
    ):
        raise AssertionError("task-monitor public integer macro totals do not add up")
    if report["handled_public_integer_macros"] != report["public_integer_macros"]:
        raise AssertionError("task-monitor omitted a LinuxCNC public integer macro")
    if report["handled_interpreter_errors"] != report["interpreter_errors"]:
        raise AssertionError("task-monitor omitted a LinuxCNC interpreter error")
    if (
        report["error_message_field_bytes"] + report["error_message_padding_bytes"]
        != report["error_message_object_bytes"]
    ):
        raise AssertionError("task-monitor error-message byte totals do not add up")
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
    if schema_fingerprint != EXPECTED_SNAPSHOT_SCHEMA_FNV64:
        raise AssertionError(
            "task-monitor snapshot fingerprint changed: "
            f"expected={EXPECTED_SNAPSHOT_SCHEMA_FNV64} actual={schema_fingerprint}"
        )
    header_fingerprint = report["public_header_source_fnv64"]
    if not isinstance(header_fingerprint, str) or not re.fullmatch(
        r"0x[0-9a-f]{16}", header_fingerprint
    ):
        raise AssertionError(
            f"invalid LinuxCNC public header fingerprint: {header_fingerprint!r}"
        )
    if int(header_fingerprint, 16) == 0:
        raise AssertionError("LinuxCNC public header fingerprint must not be zero")
    if header_fingerprint != EXPECTED_PUBLIC_HEADER_SOURCE_FNV64:
        raise AssertionError(
            "LinuxCNC public header fingerprint changed: "
            f"expected={EXPECTED_PUBLIC_HEADER_SOURCE_FNV64} actual={header_fingerprint}"
        )
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
        f"{report['public_integer_macros']} public integer macros and classifies all "
        f"{report['public_macros']} public macro names across {report['public_headers']} headers, "
        f"{report['interpreter_errors']} interpreter errors, every byte of all "
        f"{report['error_message_contracts']} error-message layouts, and every byte of its "
        f"{report['snapshot_size']}-byte status snapshot across "
        f"{report['snapshot_copy_signature_rounds']} signature rounds and derives "
        f"{report['snapshot_rust_derived_fields']} runtime fields in Rust"
    )
