"""Strict AXIS-side types for Rust script-contract inspection output."""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
import os
from pathlib import Path
import subprocess

from .recovery_contract import (
    RecoveryClassCode,
    RecoveryContract,
    recovery_contract,
)


INSPECTION_FORMAT = "DMC2_SCRIPT_CONTRACT_V1"
INSPECTION_FIELDS = (
    "format",
    "path_hex",
    "content_bytes",
    "content_fnv1a64",
    "contract_source",
    "effects",
    "prerequisites",
    "recovery_class",
    "recovery_slug",
)
INSPECTION_TIMEOUT_SECONDS = 5
U64_MAX = (1 << 64) - 1


class ScriptContractSource(Enum):
    HEADER = "header"
    CONSERVATIVE_DEFAULT = "conservative-default"


class ScriptEffect(Enum):
    AXIS_MOTION = "axis-motion"
    SPINDLE = "spindle"
    PROBE_POWER = "probe-power"
    COOLANT = "coolant"
    TOOL_CHANGE = "tool-change"
    DIGITAL_OUTPUT = "digital-output"
    COORDINATE_STATE = "coordinate-state"
    EXTERNAL_COMMAND = "external-command"
    UNCLASSIFIED_MACHINE_CODE = "unclassified-machine-code"


class ScriptPrerequisite(Enum):
    RUNNING_SESSION = "running-session"
    ESTOP_CLEAR = "estop-clear"
    MACHINE_ON = "machine-on"
    INTERPRETER_IDLE = "interpreter-idle"
    ALL_HOMED = "all-homed"


CONSERVATIVE_PREREQUISITES = tuple(ScriptPrerequisite)


@dataclass(frozen=True)
class ContentRevision:
    bytes: int
    fnv1a64: int


@dataclass(frozen=True)
class ScriptContract:
    """Closed AXIS-side representation emitted by the Rust parser."""

    path: str
    source: ScriptContractSource
    effects: tuple[ScriptEffect, ...]
    prerequisites: tuple[ScriptPrerequisite, ...]
    recovery: RecoveryContract
    revision: ContentRevision


class ScriptLoaderFailureKind(Enum):
    REQUEST_PATH_INVALID = "SCRIPT_REQUEST_PATH_INVALID"
    PATH_IDENTITY_UNAVAILABLE = "SCRIPT_PATH_IDENTITY_UNAVAILABLE"
    INSPECTOR_UNAVAILABLE = "SCRIPT_INSPECTOR_UNAVAILABLE"
    INSPECTION_TIMED_OUT = "SCRIPT_INSPECTION_TIMED_OUT"
    INSPECTION_REJECTED = "SCRIPT_INSPECTION_REJECTED"
    INSPECTION_PROTOCOL_INVALID = "SCRIPT_INSPECTION_PROTOCOL_INVALID"
    INSPECTION_PATH_MISMATCH = "SCRIPT_INSPECTION_PATH_MISMATCH"
    INSPECTION_INTEGRATION_FAILED = "SCRIPT_INSPECTION_INTEGRATION_FAILED"
    HEADER_NOT_ESTABLISHED_AT_LOAD = "SCRIPT_HEADER_NOT_ESTABLISHED_AT_LOAD"
    CONTENT_CHANGED_AFTER_LOAD = "SCRIPT_CONTENT_CHANGED_AFTER_LOAD"
    STOCK_OPEN_FAILED = "SCRIPT_STOCK_OPEN_FAILED"


class ScriptLoaderFailure(RuntimeError):
    """A closed script-boundary failure; never a machine command."""

    def __init__(self, kind: ScriptLoaderFailureKind, detail: object) -> None:
        if not isinstance(kind, ScriptLoaderFailureKind):
            raise TypeError(
                "SCRIPT_LOADER_FAILURE_KIND_INVALID: "
                f"{kind!r}; action: use a ScriptLoaderFailureKind variant"
            )
        self.kind = kind
        self.detail = detail
        super().__init__(f"{kind.value}: {detail}")


def _path_text(value: object) -> str:
    try:
        path = os.fspath(value)
    except TypeError as error:
        raise ScriptLoaderFailure(
            ScriptLoaderFailureKind.REQUEST_PATH_INVALID,
            f"file path is not path-like: {value!r}",
        ) from error
    return os.fsdecode(path)


def same_machine_file(left: object, right: object) -> bool:
    """Compare existing file identities; an unreadable identity is an error."""
    if left is None or right is None:
        return False
    left_path = _path_text(left)
    right_path = _path_text(right)
    if not left_path or not right_path:
        return False
    try:
        return os.path.samefile(left_path, right_path)
    except OSError as error:
        raise ScriptLoaderFailure(
            ScriptLoaderFailureKind.PATH_IDENTITY_UNAVAILABLE,
            f"cannot compare {left_path!r} with {right_path!r}: {error}; "
            "restore access to the files and reopen the program through AXIS File Open",
        ) from error


def _protocol_failure(detail: object) -> ScriptLoaderFailure:
    return ScriptLoaderFailure(
        ScriptLoaderFailureKind.INSPECTION_PROTOCOL_INVALID,
        detail,
    )


def _parse_enum_list(raw: str, enum_type, field: str) -> tuple:
    values = raw.split(";")
    if not raw or any(not value for value in values):
        raise _protocol_failure(f"{field} must contain non-empty semicolon values")
    if len(values) != len(set(values)):
        raise _protocol_failure(f"{field} contains a duplicate value: {raw!r}")
    try:
        return tuple(enum_type(value) for value in values)
    except ValueError as error:
        raise _protocol_failure(f"{field} contains an unknown value: {raw!r}") from error


def _parse_decimal(raw: str, field: str) -> int:
    try:
        value = int(raw, 10)
    except ValueError as error:
        raise _protocol_failure(f"{field} is not an unsigned decimal integer") from error
    if value < 0 or value > U64_MAX or str(value) != raw:
        raise _protocol_failure(f"{field} is not canonically encoded: {raw!r}")
    return value


def _parse_lower_hex(raw: str, field: str, *, exact_digits: int | None = None) -> bytes:
    if (
        not raw
        or len(raw) % 2 != 0
        or (exact_digits is not None and len(raw) != exact_digits)
        or any(character not in "0123456789abcdef" for character in raw)
    ):
        raise _protocol_failure(
            f"{field} is not canonical lowercase hexadecimal: {raw!r}"
        )
    return bytes.fromhex(raw)


def _parse_fnv1a64(raw: str) -> int:
    _parse_lower_hex(raw, "content_fnv1a64", exact_digits=16)
    return int(raw, 16)


def parse_inspection_output(output: bytes, requested_path: object) -> ScriptContract:
    """Decode only the exact machine-readable Rust inspection protocol."""
    try:
        text = output.decode("ascii")
    except UnicodeDecodeError as error:
        raise _protocol_failure("dmc2ctl inspection output is not ASCII") from error
    lines = text.splitlines()
    if len(lines) != len(INSPECTION_FIELDS):
        raise _protocol_failure(
            f"expected {len(INSPECTION_FIELDS)} fields, received {len(lines)}"
        )
    parsed: dict[str, str] = {}
    for expected, line in zip(INSPECTION_FIELDS, lines, strict=True):
        field, separator, value = line.partition("=")
        if not separator or field != expected or field in parsed:
            raise _protocol_failure(
                f"expected field {expected!r}, received {line!r}"
            )
        parsed[field] = value

    if parsed["format"] != INSPECTION_FORMAT:
        raise _protocol_failure(
            f"unsupported format {parsed['format']!r}; expected {INSPECTION_FORMAT!r}"
        )
    canonical_path = os.fsdecode(_parse_lower_hex(parsed["path_hex"], "path_hex"))
    if not same_machine_file(requested_path, canonical_path):
        raise ScriptLoaderFailure(
            ScriptLoaderFailureKind.INSPECTION_PATH_MISMATCH,
            f"requested={_path_text(requested_path)!r} inspected={canonical_path!r}",
        )
    revision = ContentRevision(
        bytes=_parse_decimal(parsed["content_bytes"], "content_bytes"),
        fnv1a64=_parse_fnv1a64(parsed["content_fnv1a64"]),
    )
    try:
        source = ScriptContractSource(parsed["contract_source"])
    except ValueError as error:
        raise _protocol_failure(
            f"unknown contract_source {parsed['contract_source']!r}"
        ) from error
    effects = _parse_enum_list(parsed["effects"], ScriptEffect, "effects")
    prerequisites = _parse_enum_list(
        parsed["prerequisites"], ScriptPrerequisite, "prerequisites"
    )
    try:
        recovery_code = RecoveryClassCode[parsed["recovery_class"]]
    except KeyError as error:
        raise _protocol_failure(
            f"unknown recovery_class {parsed['recovery_class']!r}"
        ) from error
    recovery = recovery_contract(recovery_code)
    if parsed["recovery_slug"] != recovery.slug:
        raise _protocol_failure(
            "recovery class/slug mismatch: "
            f"class={recovery_code.name!r} slug={parsed['recovery_slug']!r}"
        )
    return ScriptContract(
        path=canonical_path,
        source=source,
        effects=effects,
        prerequisites=prerequisites,
        recovery=recovery,
        revision=revision,
    )


class ScriptInspector:
    """Invoke the Rust parser without issuing a LinuxCNC command."""

    def __init__(self, executable: Path, project_root: Path) -> None:
        self.executable = executable
        self.project_root = project_root

    def inspect(self, requested_path: object) -> ScriptContract:
        path = _path_text(requested_path)
        try:
            result = subprocess.run(
                (os.fspath(self.executable), "inspect-file", path),
                cwd=self.project_root,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
                timeout=INSPECTION_TIMEOUT_SECONDS,
            )
        except subprocess.TimeoutExpired as error:
            raise ScriptLoaderFailure(
                ScriptLoaderFailureKind.INSPECTION_TIMED_OUT,
                f"path={path!r} timeout_seconds={INSPECTION_TIMEOUT_SECONDS}",
            ) from error
        except ValueError as error:
            raise ScriptLoaderFailure(
                ScriptLoaderFailureKind.REQUEST_PATH_INVALID,
                f"path={path!r} cause={error}",
            ) from error
        except OSError as error:
            raise ScriptLoaderFailure(
                ScriptLoaderFailureKind.INSPECTOR_UNAVAILABLE,
                f"executable={self.executable} cause={error}",
            ) from error
        if result.returncode != 0:
            evidence = result.stderr.decode("utf-8", errors="replace").strip()
            if not evidence:
                evidence = result.stdout.decode("utf-8", errors="replace").strip()
            raise ScriptLoaderFailure(
                ScriptLoaderFailureKind.INSPECTION_REJECTED,
                f"path={path!r} exit_code={result.returncode} evidence={evidence!r}",
            )
        if result.stderr:
            raise _protocol_failure(
                "successful dmc2ctl inspection wrote stderr: "
                + result.stderr.decode("utf-8", errors="replace").strip()
            )
        return parse_inspection_output(result.stdout, path)
