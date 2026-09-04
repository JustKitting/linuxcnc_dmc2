"""Strict reader for the Rust-owned self-describing diagnostic journal."""

from __future__ import annotations

import os
import re
import stat
from collections import deque
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import BinaryIO

from .constants import REQUIRED_LINUXCNC_VERSION
from .operation_catalog import Operation, default_catalog_path, read_operations
from .recovery_contract import (
    RECOVERY_CONTRACTS_BY_CODE,
    RecoveryClassCode,
    RecoveryOperationCode,
    RecoveryTransitionCode,
    validate_recovery_contract_model,
)


JOURNAL_SCHEMA_VERSION = 3
JOURNAL_HEADER_MARKER = "DMC2_DIAGNOSTIC_JOURNAL"
JOURNAL_RECOVERY_MARKER = "DMC2_RECOVERY_CLASS"
JOURNAL_EVENT_MARKER = "DMC2_DIAGNOSTIC_EVENT"
LINUXCNC_SOURCE_COMMIT = "86cdca76fa2a36274c432caa21952b23c267989a"
FNV64_OFFSET_BASIS = 0xCBF29CE484222325
FNV64_PRIME = 0x100000001B3
U64_MAX = (1 << 64) - 1
U32_MAX = (1 << 32) - 1
I64_MIN = -(1 << 63)
I64_MAX = (1 << 63) - 1
HEX_RE = re.compile(r"(?:[0-9a-f]{2})*")
CATEGORY_RE = re.compile(r"[0-9a-f]{16}")
KNOWN_IDENTITY_RE = re.compile(r"[A-Z][A-Z0-9_]*")
DOMAIN_RE = re.compile(r"[a-z][a-z0-9_]*")
SLUG_RE = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*")

_TRANSITIONS = {
    "ASSERT": 1,
    "CLEAR": -1,
}
_SEVERITIES = frozenset(("warning", "error"))


def default_diagnostic_journal_path() -> Path:
    return Path(__file__).resolve().parents[2] / "var/log/linuxcnc/diagnostics.tsv"


@dataclass(frozen=True)
class RecoveryRoute:
    code: RecoveryClassCode
    identity: str
    slug: str
    transition_code: RecoveryTransitionCode
    transition_identity: str
    clear_transition: str
    operation_codes: tuple[RecoveryOperationCode, ...]
    operations: tuple[Operation, ...]

    @property
    def operation_ids(self) -> tuple[str, ...]:
        return tuple(operation.value for operation in self.operation_codes)

    @property
    def ui_path(self) -> str:
        return " -> ".join(
            f"{operation.label} [{operation.ui_target}]"
            for operation in self.operations
        )


@dataclass(frozen=True)
class DiagnosticEvent:
    sequence: int
    transition: str
    transition_code: int
    severity: str
    category: int
    domain_id: int
    raw_value: int
    source: str
    domain: str
    identity: str
    known: bool
    cause: str
    operator_action: str
    recovery: RecoveryRoute
    evidence: str

    def active_key(self) -> tuple[object, ...]:
        """Return every issue field used by Rust's active-set identity."""
        return (
            self.severity,
            self.category,
            self.source,
            self.domain,
            self.domain_id,
            self.raw_value,
            self.identity,
            self.known,
            self.cause,
            self.operator_action,
            self.recovery.code,
        )

    def notification_text(self) -> str:
        """Render the operator view; retained evidence stays in the journal/log."""
        recovery_controls = " -> ".join(
            operation.label for operation in self.recovery.operations
        )
        return (
            f"{self.identity} [{self.recovery.identity}]\n"
            f"Cause: {self.cause}\n"
            f"Action: {self.operator_action}\n"
            f"Recovery: {recovery_controls}\n"
            f"Clear condition: {self.recovery.clear_transition}\n"
            f"Details: {self.source} ({self.domain}, raw={self.raw_value})"
        )


class DiagnosticJournalReader:
    """Verify and poll diagnostic transitions while retaining the active set."""

    def __init__(
        self,
        path: Path | None = None,
        operations: Mapping[str, Operation] | None = None,
    ):
        validate_recovery_contract_model()
        self.path = path or default_diagnostic_journal_path()
        self._operations = dict(operations or read_operations(default_catalog_path()))
        self._file: BinaryIO | None = None
        self._identity: tuple[int, int] | None = None
        self._buffer = b""
        self._header_seen = False
        self._expected_recovery_count = 0
        self._recovery_routes: dict[RecoveryClassCode, RecoveryRoute] = {}
        self._next_sequence = 1
        self._events: deque[DiagnosticEvent] = deque()
        self._active: dict[tuple[object, ...], DiagnosticEvent] = {}
        self._failed_identity: tuple[int, int] | None = None

    def poll(self) -> DiagnosticEvent | None:
        if self._events:
            return self._events.popleft()
        identity = self._refresh_file()
        if identity is None or identity == self._failed_identity:
            return None
        try:
            self._read_complete_lines()
        except Exception:
            self._failed_identity = identity
            raise
        if self._events:
            return self._events.popleft()
        return None

    def active_events(self) -> tuple[DiagnosticEvent, ...]:
        return tuple(
            sorted(
                self._active.values(),
                key=lambda event: (
                    event.severity != "error",
                    event.identity,
                    event.domain_id,
                    event.raw_value,
                    event.source,
                ),
            )
        )

    def recovery_routes(self) -> tuple[RecoveryRoute, ...]:
        return tuple(self._recovery_routes[code] for code in sorted(self._recovery_routes))

    def recovery_route(self, code: RecoveryClassCode) -> RecoveryRoute:
        try:
            return self._recovery_routes[code]
        except KeyError as error:
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_CLASS_NOT_LOADED: "
                f"{code.name}; action: restore the matched diagnostic journal"
            ) from error

    def contract_ready(self) -> bool:
        """Report whether the current file supplied the complete typed catalog."""
        return (
            self._file is not None
            and self._failed_identity is None
            and self._header_seen
            and self._expected_recovery_count == len(RecoveryClassCode)
            and len(self._recovery_routes) == len(RecoveryClassCode)
        )

    def close(self) -> None:
        self._close()

    def _refresh_file(self) -> tuple[int, int] | None:
        try:
            status = self.path.lstat()
        except FileNotFoundError:
            self._close()
            return None
        if not stat.S_ISREG(status.st_mode):
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_TARGET_NOT_REGULAR: "
                f"{self.path}; action: restore the configured regular journal file"
            )
        identity = (status.st_dev, status.st_ino)
        if self._file is not None and self._identity == identity:
            if status.st_size >= self._file.tell():
                return identity
        self._close()
        opened = self.path.open("rb")
        opened_status = os.fstat(opened.fileno())
        opened_identity = (opened_status.st_dev, opened_status.st_ino)
        if not stat.S_ISREG(opened_status.st_mode) or opened_identity != identity:
            opened.close()
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_CHANGED_DURING_OPEN: journal identity changed; "
                "action: preserve the file and restart the AXIS reader"
            )
        self._file = opened
        self._identity = identity
        self._failed_identity = None
        self._buffer = b""
        self._header_seen = False
        self._expected_recovery_count = 0
        self._recovery_routes.clear()
        self._next_sequence = 1
        self._events.clear()
        self._active.clear()
        return identity

    def _close(self) -> None:
        if self._file is not None:
            self._file.close()
        self._file = None
        self._identity = None
        self._buffer = b""
        self._header_seen = False
        self._expected_recovery_count = 0
        self._recovery_routes.clear()
        self._next_sequence = 1
        self._events.clear()
        self._active.clear()

    def _read_complete_lines(self) -> None:
        if self._file is None:
            return
        self._buffer += self._file.read()
        while b"\n" in self._buffer:
            raw_line, self._buffer = self._buffer.split(b"\n", 1)
            try:
                line = raw_line.decode("ascii")
            except UnicodeDecodeError as error:
                raise RuntimeError(
                    "DIAGNOSTIC_JOURNAL_NON_ASCII_FRAMING: framing is not ASCII; "
                    "action: preserve the journal and restart the task monitor"
                ) from error
            if not self._header_seen:
                self._validate_header(line)
                self._header_seen = True
                continue
            if len(self._recovery_routes) < self._expected_recovery_count:
                recovery = self._parse_recovery(line)
                if recovery.code in self._recovery_routes:
                    raise RuntimeError(
                        "DIAGNOSTIC_RECOVERY_CLASS_DUPLICATE_CODE: "
                        f"{recovery.code}; action: run matched DMC2 binaries"
                    )
                if any(
                    current.identity == recovery.identity
                    for current in self._recovery_routes.values()
                ):
                    raise RuntimeError(
                        "DIAGNOSTIC_RECOVERY_CLASS_DUPLICATE_IDENTITY: "
                        f"{recovery.identity}; action: run matched DMC2 binaries"
                    )
                self._recovery_routes[recovery.code] = recovery
                if len(self._recovery_routes) == self._expected_recovery_count:
                    expected_codes = set(RecoveryClassCode)
                    if set(self._recovery_routes) != expected_codes:
                        raise RuntimeError(
                            "DIAGNOSTIC_RECOVERY_CLASS_CODE_SET_INVALID: "
                            f"expected={[code.value for code in sorted(expected_codes)]!r} "
                            f"actual={[code.value for code in sorted(self._recovery_routes)]!r}; "
                            "action: run matched DMC2 binaries"
                        )
                continue
            event = self._parse_event(line)
            if event.sequence != self._next_sequence:
                raise RuntimeError(
                    "DIAGNOSTIC_JOURNAL_SEQUENCE_MISMATCH: "
                    f"expected {self._next_sequence}, found {event.sequence}; "
                    "action: preserve the journal and restart the task monitor"
                )
            self._next_sequence += 1
            self._apply_transition(event)
            self._events.append(event)

    def _validate_header(self, line: str) -> None:
        expected = (
            JOURNAL_HEADER_MARKER,
            str(JOURNAL_SCHEMA_VERSION),
            REQUIRED_LINUXCNC_VERSION,
            LINUXCNC_SOURCE_COMMIT,
        )
        fields = tuple(line.split("\t"))
        if len(fields) != 5 or fields[:4] != expected:
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_HEADER_MISMATCH: "
                f"expected={expected!r} actual={fields!r}; "
                "action: run the matched DMC2 and LinuxCNC 2.9.10 binaries"
            )
        self._expected_recovery_count = _bounded_int(
            fields[4], "recovery class count", 1, 255
        )
        if self._expected_recovery_count != len(RecoveryClassCode):
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_CLASS_COUNT_MISMATCH: "
                f"producer={self._expected_recovery_count} "
                f"consumer={len(RecoveryClassCode)}; action: run matched DMC2 binaries"
            )

    def _parse_recovery(self, line: str) -> RecoveryRoute:
        fields = line.split("\t")
        if len(fields) != 10:
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_CLASS_FIELD_COUNT_MISMATCH: "
                f"found {len(fields)}, expected 10; action: run matched DMC2 binaries"
            )
        if fields[0] != JOURNAL_RECOVERY_MARKER or fields[1] != str(
            JOURNAL_SCHEMA_VERSION
        ):
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_CLASS_SCHEMA_MISMATCH: "
                "action: run matched DMC2 binaries"
            )
        prefix = "\t".join(fields[:9]).encode("ascii")
        expected_checksum = f"{_fnv64(prefix):016x}"
        if fields[9] != expected_checksum:
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_CLASS_CHECKSUM_MISMATCH: "
                f"expected {expected_checksum}, found {fields[9]}; "
                "action: preserve the journal and inspect storage integrity"
            )
        raw_code = _bounded_int(fields[2], "recovery class code", 1, 255)
        try:
            code = RecoveryClassCode(raw_code)
        except ValueError as error:
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_CLASS_UNKNOWN: "
                f"{raw_code}; action: run matched DMC2 binaries"
            ) from error
        identity = _utf8_hex(fields[3], "recovery class identity")
        slug = _utf8_hex(fields[4], "recovery class slug")
        raw_transition = _bounded_int(fields[5], "recovery transition code", 1, 255)
        try:
            transition_code = RecoveryTransitionCode(raw_transition)
        except ValueError as error:
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_TRANSITION_UNKNOWN: "
                f"{raw_transition}; action: run matched DMC2 binaries"
            ) from error
        transition_identity = _utf8_hex(fields[6], "recovery transition identity")
        clear_transition = _utf8_hex(fields[7], "recovery clear condition")
        operation_ids = tuple(
            value
            for value in _utf8_hex(fields[8], "recovery UI operations").split(";")
            if value
        )
        try:
            operation_codes = tuple(
                RecoveryOperationCode(operation_id)
                for operation_id in operation_ids
            )
        except ValueError as error:
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_UI_OPERATION_UNKNOWN: "
                f"class={identity} operations={operation_ids!r}; "
                "action: run matched DMC2 binaries"
            ) from error
        expected_contract = RECOVERY_CONTRACTS_BY_CODE[code]
        expected_transition = expected_contract.transition_code
        expected_operation_ids = expected_contract.operation_ids
        if identity != code.name:
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_CLASS_IDENTITY_MISMATCH: "
                f"code={code.value} expected={code.name!r} actual={identity!r}; "
                "action: run matched DMC2 binaries"
            )
        if slug != expected_contract.slug:
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_CLASS_SLUG_MISMATCH: "
                f"class={identity} expected={expected_contract.slug!r} actual={slug!r}; "
                "action: run matched DMC2 binaries"
            )
        if transition_code != expected_transition or transition_identity != transition_code.name:
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_TRANSITION_MISMATCH: "
                f"class={identity} expected={expected_transition.name} "
                f"actual={transition_identity}; action: run matched DMC2 binaries"
            )
        if clear_transition != expected_contract.clear_transition:
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_CLEAR_TRANSITION_MISMATCH: "
                f"class={identity}; action: run matched DMC2 binaries"
            )
        if (
            KNOWN_IDENTITY_RE.fullmatch(identity) is None
            or KNOWN_IDENTITY_RE.fullmatch(transition_identity) is None
            or SLUG_RE.fullmatch(slug) is None
            or not clear_transition
            or not operation_ids
        ):
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_CLASS_CONTRACT_INCOMPLETE: "
                f"{identity}; action: run a corrected DMC2 binary"
            )
        if len(set(operation_ids)) != len(operation_ids):
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_CLASS_DUPLICATE_UI_OPERATION: "
                f"{identity}; action: correct the recovery class definition"
            )
        if operation_ids != expected_operation_ids:
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_UI_PATH_MISMATCH: "
                f"class={identity} expected={expected_operation_ids!r} "
                f"actual={operation_ids!r}; action: run matched DMC2 binaries"
            )
        missing = tuple(
            operation_id
            for operation_id in operation_ids
            if operation_id not in self._operations
        )
        if missing:
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_UI_OPERATION_MISSING: "
                f"class={identity} operations={missing!r}; "
                "action: restore the matched operation catalog"
            )
        operations = tuple(self._operations[operation_id] for operation_id in operation_ids)
        if any(not operation.ui_target for operation in operations):
            raise RuntimeError(
                "DIAGNOSTIC_RECOVERY_UI_TARGET_MISSING: "
                f"class={identity}; action: correct the operation catalog"
            )
        return RecoveryRoute(
            code=code,
            identity=identity,
            slug=slug,
            transition_code=transition_code,
            transition_identity=transition_identity,
            clear_transition=clear_transition,
            operation_codes=operation_codes,
            operations=operations,
        )

    def _apply_transition(self, event: DiagnosticEvent) -> None:
        key = event.active_key()
        if event.transition == "ASSERT":
            if key in self._active:
                raise RuntimeError(
                    "DIAGNOSTIC_JOURNAL_DUPLICATE_ASSERT: "
                    f"{event.identity} was already active; "
                    "action: preserve the journal and restart the task monitor"
                )
            self._active[key] = event
            return
        if key not in self._active:
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_CLEAR_WITHOUT_ASSERT: "
                f"{event.identity} was not active; "
                "action: preserve the journal and restart the task monitor"
            )
        del self._active[key]

    def _parse_event(self, line: str) -> DiagnosticEvent:
        fields = line.split("\t")
        if len(fields) != 18:
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_FIELD_COUNT_MISMATCH: "
                f"found {len(fields)}, expected 18; "
                "action: preserve the journal and restart the task monitor"
            )
        if fields[0] != JOURNAL_EVENT_MARKER or fields[1] != str(
            JOURNAL_SCHEMA_VERSION
        ):
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_EVENT_SCHEMA_MISMATCH: marker or schema changed; "
                "action: run matched DMC2 binaries"
            )
        prefix = "\t".join(fields[:17]).encode("ascii")
        expected_checksum = f"{_fnv64(prefix):016x}"
        if fields[17] != expected_checksum:
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_CHECKSUM_MISMATCH: "
                f"expected {expected_checksum}, found {fields[17]}; "
                "action: preserve the journal and inspect storage integrity"
            )

        sequence = _bounded_int(fields[2], "sequence", 1, U64_MAX)
        transition = fields[3]
        transition_code = _bounded_int(fields[4], "transition code", -1, 1)
        expected_transition_code = _TRANSITIONS.get(transition)
        if expected_transition_code is None or transition_code != expected_transition_code:
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_TRANSITION_MISMATCH: "
                f"name={transition!r} code={transition_code}; "
                "action: run matched DMC2 binaries"
            )
        severity = fields[5]
        if severity not in _SEVERITIES:
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_SEVERITY_UNKNOWN: "
                f"{severity!r}; action: run matched DMC2 binaries"
            )
        if CATEGORY_RE.fullmatch(fields[6]) is None:
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_CATEGORY_INVALID: "
                f"{fields[6]!r}; action: preserve the journal"
            )
        category = int(fields[6], 16)
        if category == 0:
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_CATEGORY_ZERO: diagnostics require a category; "
                "action: run a corrected task monitor"
            )
        raw_recovery_code = _bounded_int(fields[7], "recovery class code", 1, 255)
        try:
            recovery_code = RecoveryClassCode(raw_recovery_code)
        except ValueError as error:
            raise RuntimeError(
                "DIAGNOSTIC_EVENT_RECOVERY_CLASS_UNKNOWN: "
                f"{raw_recovery_code}; action: run matched DMC2 binaries"
            ) from error
        recovery = self._recovery_routes.get(recovery_code)
        if recovery is None:
            raise RuntimeError(
                "DIAGNOSTIC_EVENT_RECOVERY_CLASS_UNKNOWN: "
                f"{recovery_code.value}; action: run matched DMC2 binaries"
            )
        domain_id = _bounded_int(fields[8], "domain id", 0, U32_MAX)
        raw_value = _bounded_int(fields[9], "raw value", I64_MIN, I64_MAX)
        source = _utf8_hex(fields[10], "source")
        domain = _utf8_hex(fields[11], "domain")
        identity = _utf8_hex(fields[12], "identity")
        if fields[13] not in ("0", "1"):
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_KNOWN_FLAG_INVALID: "
                f"{fields[13]!r}; action: preserve the journal"
            )
        known = fields[13] == "1"
        cause = _utf8_hex(fields[14], "cause")
        operator_action = _utf8_hex(fields[15], "operator action")
        evidence = _utf8_hex(fields[16], "evidence")
        if (
            not source
            or not domain
            or not identity
            or not cause
            or not operator_action
            or not evidence
        ):
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_DESCRIPTION_MISSING: identity, cause, action, "
                "evidence, source, and domain must all be present; "
                "action: run a corrected task monitor"
            )
        if DOMAIN_RE.fullmatch(domain) is None:
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_DOMAIN_INVALID: "
                f"{domain!r}; action: preserve the journal and run matched DMC2 binaries"
            )
        if known:
            if KNOWN_IDENTITY_RE.fullmatch(identity) is None:
                raise RuntimeError(
                    "DIAGNOSTIC_JOURNAL_KNOWN_IDENTITY_INVALID: "
                    f"{identity!r}; action: run matched DMC2 binaries"
                )
        else:
            expected_identity = f"UNKNOWN_{domain.upper()}(raw={raw_value})"
            if identity != expected_identity:
                raise RuntimeError(
                    "DIAGNOSTIC_JOURNAL_UNKNOWN_IDENTITY_MISMATCH: "
                    f"expected {expected_identity!r}, found {identity!r}; "
                    "action: preserve the raw value and run a corrected task monitor"
                )

        return DiagnosticEvent(
            sequence=sequence,
            transition=transition,
            transition_code=transition_code,
            severity=severity,
            category=category,
            domain_id=domain_id,
            raw_value=raw_value,
            source=source,
            domain=domain,
            identity=identity,
            known=known,
            cause=cause,
            operator_action=operator_action,
            recovery=recovery,
            evidence=evidence,
        )


def _bounded_int(value: str, label: str, minimum: int, maximum: int) -> int:
    try:
        parsed = int(value, 10)
    except ValueError as error:
        raise RuntimeError(
            f"DIAGNOSTIC_JOURNAL_INTEGER_INVALID: {label}={value!r}; "
            "action: preserve the journal"
        ) from error
    if not minimum <= parsed <= maximum:
        raise RuntimeError(
            f"DIAGNOSTIC_JOURNAL_INTEGER_OUT_OF_RANGE: {label}={parsed} "
            f"outside [{minimum}, {maximum}]; action: preserve the journal"
        )
    return parsed


def _utf8_hex(value: str, label: str) -> str:
    if HEX_RE.fullmatch(value) is None:
        raise RuntimeError(
            f"DIAGNOSTIC_JOURNAL_HEX_INVALID: {label}; action: preserve the journal"
        )
    try:
        return bytes.fromhex(value).decode("utf-8")
    except UnicodeDecodeError as error:
        raise RuntimeError(
            f"DIAGNOSTIC_JOURNAL_UTF8_INVALID: {label}; action: preserve the journal"
        ) from error


def _fnv64(data: bytes) -> int:
    result = FNV64_OFFSET_BASIS
    for byte in data:
        result = ((result ^ byte) * FNV64_PRIME) & U64_MAX
    return result
