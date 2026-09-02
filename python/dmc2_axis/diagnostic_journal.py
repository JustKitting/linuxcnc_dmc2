"""Strict reader for the Rust-owned self-describing diagnostic journal."""

from __future__ import annotations

import os
import re
import stat
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import BinaryIO

from .constants import REQUIRED_LINUXCNC_VERSION


JOURNAL_SCHEMA_VERSION = 2
JOURNAL_HEADER_MARKER = "DMC2_DIAGNOSTIC_JOURNAL"
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

_TRANSITIONS = {
    "ASSERT": 1,
    "CLEAR": -1,
}
_SEVERITIES = frozenset(("warning", "error"))


def default_diagnostic_journal_path() -> Path:
    return Path(__file__).resolve().parents[2] / "var/log/linuxcnc/diagnostics.tsv"


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
            self.evidence,
        )

    def notification_text(self) -> str:
        return (
            f"{self.identity}\n"
            f"Cause: {self.cause}\n"
            f"Action: {self.operator_action}\n"
            f"Evidence: {self.evidence}\n"
            f"Source: {self.source} "
            f"(domain={self.domain}, raw={self.raw_value})"
        )


class DiagnosticJournalReader:
    """Verify and poll diagnostic transitions while retaining the active set."""

    def __init__(self, path: Path | None = None):
        self.path = path or default_diagnostic_journal_path()
        self._file: BinaryIO | None = None
        self._identity: tuple[int, int] | None = None
        self._buffer = b""
        self._header_seen = False
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

    def close(self) -> None:
        self._close()

    def __del__(self):
        try:
            self._close()
        except Exception:
            pass

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

    @staticmethod
    def _validate_header(line: str) -> None:
        expected = (
            JOURNAL_HEADER_MARKER,
            str(JOURNAL_SCHEMA_VERSION),
            REQUIRED_LINUXCNC_VERSION,
            LINUXCNC_SOURCE_COMMIT,
        )
        fields = tuple(line.split("\t"))
        if fields != expected:
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_HEADER_MISMATCH: "
                f"expected={expected!r} actual={fields!r}; "
                "action: run the matched DMC2 and LinuxCNC 2.9.10 binaries"
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

    @staticmethod
    def _parse_event(line: str) -> DiagnosticEvent:
        fields = line.split("\t")
        if len(fields) != 17:
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_FIELD_COUNT_MISMATCH: "
                f"found {len(fields)}, expected 17; "
                "action: preserve the journal and restart the task monitor"
            )
        if fields[0] != JOURNAL_EVENT_MARKER or fields[1] != str(
            JOURNAL_SCHEMA_VERSION
        ):
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_EVENT_SCHEMA_MISMATCH: marker or schema changed; "
                "action: run matched DMC2 binaries"
            )
        prefix = "\t".join(fields[:16]).encode("ascii")
        expected_checksum = f"{_fnv64(prefix):016x}"
        if fields[16] != expected_checksum:
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_CHECKSUM_MISMATCH: "
                f"expected {expected_checksum}, found {fields[16]}; "
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
        domain_id = _bounded_int(fields[7], "domain id", 0, U32_MAX)
        raw_value = _bounded_int(fields[8], "raw value", I64_MIN, I64_MAX)
        source = _utf8_hex(fields[9], "source")
        domain = _utf8_hex(fields[10], "domain")
        identity = _utf8_hex(fields[11], "identity")
        if fields[12] not in ("0", "1"):
            raise RuntimeError(
                "DIAGNOSTIC_JOURNAL_KNOWN_FLAG_INVALID: "
                f"{fields[12]!r}; action: preserve the journal"
            )
        known = fields[12] == "1"
        cause = _utf8_hex(fields[13], "cause")
        operator_action = _utf8_hex(fields[14], "operator action")
        evidence = _utf8_hex(fields[15], "evidence")
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
