"""Lossless presentation reader for the Rust-owned LinuxCNC error journal."""

from __future__ import annotations

import os
import re
import stat
import sys
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import BinaryIO

from .constants import ERROR_CHANNEL_KIND_DEFINITIONS, REQUIRED_LINUXCNC_VERSION


JOURNAL_SCHEMA_VERSION = 2
JOURNAL_OBJECT_CAPACITY = 280
JOURNAL_HEADER_MARKER = "DMC2_ERROR_JOURNAL"
JOURNAL_EVENT_MARKER = "DMC2_ERROR_EVENT"
LINUXCNC_SOURCE_COMMIT = "86cdca76fa2a36274c432caa21952b23c267989a"
FNV64_OFFSET_BASIS = 0xCBF29CE484222325
FNV64_PRIME = 0x100000001B3
HEX_RE = re.compile(r"(?:[0-9a-f]{2})*")
U64_MAX = (1 << 64) - 1
I32_MIN = -(1 << 31)
I32_MAX = (1 << 31) - 1

_JOURNAL_CLASS_NAMES = {
    1: "NML_ERROR",
    2: "NML_TEXT",
    3: "NML_DISPLAY",
    11: "EMC_OPERATOR_ERROR",
    12: "EMC_OPERATOR_TEXT",
    13: "EMC_OPERATOR_DISPLAY",
}
_JOURNAL_SEVERITIES = {
    value: severity
    for _public_name, value, severity in ERROR_CHANNEL_KIND_DEFINITIONS
}


def default_error_journal_path() -> Path:
    return Path(__file__).resolve().parents[2] / "var/log/linuxcnc/error-channel.tsv"


@dataclass(frozen=True)
class ErrorJournalEvent:
    sequence: int
    message_type: int
    class_name: str
    severity: str
    known: bool
    object_size: int
    declared_size: int
    serial_number: int | None
    operator_id: int | None
    payload: bytes
    text: bytes
    padding: bytes
    object_bytes: bytes
    nml_error: int
    cms_status: int

    def display_text(self) -> str:
        if not self.known:
            return (
                "UNKNOWN_LINUXCNC_ERROR_CHANNEL_TYPE"
                f"(raw={self.message_type})\n"
                "Cause: LinuxCNC supplied an error-channel type outside the pinned "
                "2.9.10 public catalog\n"
                "Action: preserve the raw journal record and stop using the machine "
                "until the binary/version mismatch is corrected"
            )
        return self.text.decode("utf-8", errors="replace")


class ErrorJournalReader:
    """Poll complete records without ever opening LinuxCNC's NML queue."""

    def __init__(self, path: Path | None = None):
        self.path = path or default_error_journal_path()
        self._file: BinaryIO | None = None
        self._identity: tuple[int, int] | None = None
        self._buffer = b""
        self._header_seen = False
        self._success_transport: tuple[int, int] | None = None
        self._next_sequence = 1
        self._events: deque[ErrorJournalEvent] = deque()
        self._failed_identity: tuple[int, int] | None = None

    def poll(self) -> ErrorJournalEvent | None:
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
            raise RuntimeError(f"error journal is not a regular file: {self.path}")
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
            raise RuntimeError("error journal changed while it was being opened")
        self._file = opened
        self._identity = identity
        self._failed_identity = None
        self._buffer = b""
        self._header_seen = False
        self._success_transport = None
        self._next_sequence = 1
        self._events.clear()
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

    def _read_complete_lines(self) -> None:
        if self._file is None:
            return
        self._buffer += self._file.read()
        while b"\n" in self._buffer:
            raw_line, self._buffer = self._buffer.split(b"\n", 1)
            try:
                line = raw_line.decode("ascii")
            except UnicodeDecodeError as error:
                raise RuntimeError("error journal contains non-ASCII framing") from error
            if not self._header_seen:
                self._success_transport = self._validate_header(line)
                self._header_seen = True
            else:
                if self._success_transport is None:
                    raise RuntimeError("error journal transport contract is unavailable")
                event = self._parse_event(line, *self._success_transport)
                if event.sequence != self._next_sequence:
                    raise RuntimeError(
                        "error journal sequence changed: "
                        f"expected {self._next_sequence}, found {event.sequence}"
                    )
                self._next_sequence += 1
                self._events.append(event)

    @staticmethod
    def _validate_header(line: str) -> tuple[int, int]:
        fields = tuple(line.split("\t"))
        if len(fields) != 9:
            raise RuntimeError(
                f"error journal header has {len(fields)} fields instead of 9"
            )
        expected_prefix = (
            JOURNAL_HEADER_MARKER,
            str(JOURNAL_SCHEMA_VERSION),
            REQUIRED_LINUXCNC_VERSION,
            LINUXCNC_SOURCE_COMMIT,
            str(JOURNAL_OBJECT_CAPACITY),
            sys.byteorder,
        )
        if fields[:6] != expected_prefix:
            raise RuntimeError(
                "error journal header changed: "
                f"expected_prefix={expected_prefix!r} actual={fields!r}"
            )
        prefix = "\t".join(fields[:8]).encode("ascii")
        expected_checksum = f"{_fnv64(prefix):016x}"
        if fields[8] != expected_checksum:
            raise RuntimeError(
                "error journal header checksum mismatch: "
                f"expected {expected_checksum}, found {fields[8]}"
            )
        nml_no_error = _bounded_int(fields[6], "NML success code", I32_MIN, I32_MAX)
        cms_read_ok = _bounded_int(fields[7], "CMS read-success code", I32_MIN, I32_MAX)
        return nml_no_error, cms_read_ok

    @staticmethod
    def _parse_event(
        line: str, nml_no_error: int, cms_read_ok: int
    ) -> ErrorJournalEvent:
        fields = line.split("\t")
        if len(fields) != 18:
            raise RuntimeError(
                f"error journal event has {len(fields)} fields instead of 18"
            )
        if fields[0] != JOURNAL_EVENT_MARKER or fields[1] != str(
            JOURNAL_SCHEMA_VERSION
        ):
            raise RuntimeError("error journal event marker or schema changed")
        prefix = "\t".join(fields[:17]).encode("ascii")
        expected_checksum = f"{_fnv64(prefix):016x}"
        if fields[17] != expected_checksum:
            raise RuntimeError(
                "error journal checksum mismatch: "
                f"expected {expected_checksum}, found {fields[17]}"
            )

        sequence = _bounded_int(fields[2], "sequence", 1, U64_MAX)
        message_type = _bounded_int(fields[3], "message type", I32_MIN, I32_MAX)
        class_name = fields[4]
        severity = fields[5]
        if fields[6] not in ("0", "1"):
            raise RuntimeError(f"invalid error journal known flag: {fields[6]!r}")
        known = fields[6] == "1"
        object_size = _positive_int(fields[7], "object size")
        declared_size = _positive_int(fields[8], "declared size")
        serial_number = _optional_i32(fields[9], "serial number")
        operator_id = _optional_i32(fields[10], "operator id")
        payload = _hex(fields[11], "payload")
        text = _hex(fields[12], "text")
        padding = _hex(fields[13], "padding")
        object_bytes = _hex(fields[14], "object")
        nml_error = _bounded_int(fields[15], "NML error", I32_MIN, I32_MAX)
        cms_status = _bounded_int(fields[16], "CMS status", I32_MIN, I32_MAX)

        if nml_error != nml_no_error or cms_status != cms_read_ok:
            raise RuntimeError(
                "ERROR_JOURNAL_TRANSPORT_STATE_INVALID: error journal message "
                "transport state changed; "
                f"expected=NML_NO_ERROR(raw={nml_no_error}),"
                f"CMS_READ_OK(raw={cms_read_ok}) "
                f"observed=nml_error(raw={nml_error}),cms_status(raw={cms_status}); "
                "cause: a record was published without a successful pinned LinuxCNC "
                "NML/CMS read; action: preserve both journals and restart only after "
                "correcting the task monitor or binary/version mismatch"
            )

        if object_size > JOURNAL_OBJECT_CAPACITY or len(object_bytes) != object_size:
            raise RuntimeError("error journal object length disagrees with object size")
        if declared_size != object_size:
            raise RuntimeError("error journal declared size disagrees with object size")
        if object_size < 16:
            raise RuntimeError("error journal object is smaller than NMLmsg")
        raw_type = int.from_bytes(object_bytes[0:4], sys.byteorder, signed=True)
        raw_size = int.from_bytes(object_bytes[8:16], sys.byteorder, signed=True)
        if raw_type != message_type or raw_size != declared_size:
            raise RuntimeError("error journal base NMLmsg fields disagree with the record")

        expected_class = _JOURNAL_CLASS_NAMES.get(message_type)
        expected_severity = _JOURNAL_SEVERITIES.get(message_type)
        if expected_class is None:
            if known or class_name != "UNKNOWN" or severity != "error":
                raise RuntimeError("unknown error journal type was mislabeled")
            if payload or text or serial_number is not None or operator_id is not None:
                raise RuntimeError("unknown error journal type invented interpreted fields")
            if padding != object_bytes[4:8]:
                raise RuntimeError("unknown error journal base padding changed")
        else:
            if not known or class_name != expected_class or severity != expected_severity:
                raise RuntimeError("known error journal type classification changed")
            operator_message = message_type >= 11
            expected_size = 280 if operator_message else 272
            payload_offset = 24 if operator_message else 16
            payload_size = 255 if operator_message else 256
            if object_size != expected_size or len(payload) != payload_size:
                raise RuntimeError("known error journal layout size changed")
            if payload != object_bytes[payload_offset : payload_offset + payload_size]:
                raise RuntimeError("error journal payload disagrees with the raw object")
            expected_text = payload.split(b"\0", 1)[0]
            if text != expected_text:
                raise RuntimeError("error journal text disagrees with its payload")
            if operator_message:
                raw_serial = int.from_bytes(
                    object_bytes[16:20], sys.byteorder, signed=True
                )
                raw_id = int.from_bytes(
                    object_bytes[20:24], sys.byteorder, signed=True
                )
                if serial_number != raw_serial or operator_id != raw_id:
                    raise RuntimeError(
                        "error journal operator metadata disagrees with the raw object"
                    )
                expected_padding = object_bytes[4:8] + object_bytes[279:280]
            else:
                if serial_number is not None or operator_id is not None:
                    raise RuntimeError("generic NML error invented operator metadata")
                expected_padding = object_bytes[4:8]
            if padding != expected_padding:
                raise RuntimeError("error journal padding disagrees with the raw object")

        return ErrorJournalEvent(
            sequence=sequence,
            message_type=message_type,
            class_name=class_name,
            severity=severity,
            known=known,
            object_size=object_size,
            declared_size=declared_size,
            serial_number=serial_number,
            operator_id=operator_id,
            payload=payload,
            text=text,
            padding=padding,
            object_bytes=object_bytes,
            nml_error=nml_error,
            cms_status=cms_status,
        )


def _int(value: str, label: str) -> int:
    try:
        return int(value, 10)
    except ValueError as error:
        raise RuntimeError(f"invalid error journal {label}: {value!r}") from error


def _positive_int(value: str, label: str) -> int:
    parsed = _int(value, label)
    if parsed <= 0:
        raise RuntimeError(f"error journal {label} must be positive: {parsed}")
    return parsed


def _bounded_int(value: str, label: str, minimum: int, maximum: int) -> int:
    parsed = _int(value, label)
    if not minimum <= parsed <= maximum:
        raise RuntimeError(
            f"error journal {label} is outside [{minimum}, {maximum}]: {parsed}"
        )
    return parsed


def _optional_i32(value: str, label: str) -> int | None:
    return (
        None
        if value == "-"
        else _bounded_int(value, label, I32_MIN, I32_MAX)
    )


def _hex(value: str, label: str) -> bytes:
    if HEX_RE.fullmatch(value) is None:
        raise RuntimeError(f"invalid error journal {label} hex")
    return bytes.fromhex(value)


def _fnv64(data: bytes) -> int:
    result = FNV64_OFFSET_BASIS
    for byte in data:
        result = ((result ^ byte) * FNV64_PRIME) & 0xFFFFFFFFFFFFFFFF
    return result
