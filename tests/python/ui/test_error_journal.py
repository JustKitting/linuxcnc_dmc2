from __future__ import annotations

import os
import sys
import tempfile
import unittest
from pathlib import Path

from dmc2_axis.error_journal import (
    FNV64_OFFSET_BASIS,
    FNV64_PRIME,
    JOURNAL_EVENT_MARKER,
    JOURNAL_HEADER_MARKER,
    JOURNAL_OBJECT_CAPACITY,
    JOURNAL_SCHEMA_VERSION,
    LINUXCNC_SOURCE_COMMIT,
    ErrorJournalReader,
)


KINDS = {
    1: ("NML_ERROR", "error", 272, 16, 256, False),
    2: ("NML_TEXT", "info", 272, 16, 256, False),
    3: ("NML_DISPLAY", "info", 272, 16, 256, False),
    11: ("EMC_OPERATOR_ERROR", "error", 280, 24, 255, True),
    12: ("EMC_OPERATOR_TEXT", "info", 280, 24, 255, True),
    13: ("EMC_OPERATOR_DISPLAY", "info", 280, 24, 255, True),
}


def fnv64(data: bytes) -> int:
    result = FNV64_OFFSET_BASIS
    for byte in data:
        result = ((result ^ byte) * FNV64_PRIME) & 0xFFFFFFFFFFFFFFFF
    return result


def header() -> bytes:
    return (
        f"{JOURNAL_HEADER_MARKER}\t{JOURNAL_SCHEMA_VERSION}\t2.9.10\t"
        f"{LINUXCNC_SOURCE_COMMIT}\t{JOURNAL_OBJECT_CAPACITY}\t{sys.byteorder}\n"
    ).encode("ascii")


def event_line(
    sequence: int,
    message_type: int,
    text: bytes = b"message",
    *,
    mutate=None,
) -> bytes:
    class_name, severity, size, payload_offset, payload_size, operator = KINDS[
        message_type
    ]
    object_bytes = bytearray(size)
    object_bytes[0:4] = message_type.to_bytes(4, sys.byteorder, signed=True)
    object_bytes[8:16] = size.to_bytes(8, sys.byteorder, signed=True)
    serial = 0x10203040 if operator else None
    operator_id = -123 if operator else None
    if operator:
        object_bytes[16:20] = serial.to_bytes(4, sys.byteorder, signed=True)
        object_bytes[20:24] = operator_id.to_bytes(4, sys.byteorder, signed=True)
    payload = bytearray(payload_size)
    payload[: len(text)] = text
    payload[len(text)] = 0
    object_bytes[payload_offset : payload_offset + payload_size] = payload
    padding = (
        object_bytes[4:8] + object_bytes[279:280]
        if operator
        else object_bytes[4:8]
    )
    fields = [
        JOURNAL_EVENT_MARKER,
        str(JOURNAL_SCHEMA_VERSION),
        str(sequence),
        str(message_type),
        class_name,
        severity,
        "1",
        str(size),
        str(size),
        str(serial) if serial is not None else "-",
        str(operator_id) if operator_id is not None else "-",
        bytes(payload).hex(),
        text.hex(),
        bytes(padding).hex(),
        bytes(object_bytes).hex(),
        "0",
        "1",
    ]
    if mutate is not None:
        mutate(fields)
    prefix = "\t".join(fields)
    return f"{prefix}\t{fnv64(prefix.encode('ascii')):016x}\n".encode("ascii")


def unknown_event_line(sequence: int, message_type: int = 77, size: int = 32) -> bytes:
    object_bytes = bytearray(range(size))
    object_bytes[0:4] = message_type.to_bytes(4, sys.byteorder, signed=True)
    object_bytes[8:16] = size.to_bytes(8, sys.byteorder, signed=True)
    fields = [
        JOURNAL_EVENT_MARKER,
        str(JOURNAL_SCHEMA_VERSION),
        str(sequence),
        str(message_type),
        "UNKNOWN",
        "error",
        "0",
        str(size),
        str(size),
        "-",
        "-",
        "",
        "",
        bytes(object_bytes[4:8]).hex(),
        bytes(object_bytes).hex(),
        "0",
        "1",
    ]
    prefix = "\t".join(fields)
    return f"{prefix}\t{fnv64(prefix.encode('ascii')):016x}\n".encode("ascii")


class ErrorJournalReaderTests(unittest.TestCase):
    def write(self, path: Path, *lines: bytes) -> None:
        path.write_bytes(header() + b"".join(lines))

    def test_all_six_source_types_are_parsed_with_complete_raw_layouts(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "errors.tsv"
            self.write(
                path,
                *(event_line(index, kind) for index, kind in enumerate(KINDS, 1)),
            )
            reader = ErrorJournalReader(path)
            for sequence, message_type in enumerate(KINDS, 1):
                event = reader.poll()
                self.assertIsNotNone(event)
                self.assertEqual(event.sequence, sequence)
                self.assertEqual(event.message_type, message_type)
                self.assertEqual(event.class_name, KINDS[message_type][0])
                self.assertEqual(event.severity, KINDS[message_type][1])
                self.assertEqual(event.display_text(), "message")
                self.assertEqual(len(event.object_bytes), KINDS[message_type][2])
                self.assertEqual(len(event.payload), KINDS[message_type][4])
                self.assertEqual(len(event.padding), 5 if message_type >= 11 else 4)
                if message_type >= 11:
                    self.assertEqual(event.serial_number, 0x10203040)
                    self.assertEqual(event.operator_id, -123)
                else:
                    self.assertIsNone(event.serial_number)
                    self.assertIsNone(event.operator_id)
            self.assertIsNone(reader.poll())

    def test_missing_and_partial_files_are_non_events_until_a_record_is_complete(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "errors.tsv"
            reader = ErrorJournalReader(path)
            self.assertIsNone(reader.poll())
            line = event_line(1, 1)
            path.write_bytes(header() + line[:-1])
            self.assertIsNone(reader.poll())
            with path.open("ab") as output:
                output.write(b"\n")
            self.assertEqual(reader.poll().message_type, 1)

    def test_atomic_file_replacement_starts_a_new_sequence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "errors.tsv"
            self.write(path, event_line(1, 1))
            reader = ErrorJournalReader(path)
            self.assertEqual(reader.poll().message_type, 1)

            replacement = root / "replacement.tsv"
            self.write(replacement, event_line(1, 13))
            os.replace(replacement, path)
            self.assertEqual(reader.poll().message_type, 13)

    def test_checksum_header_sequence_and_every_redundant_byte_view_are_enforced(self):
        mutations = (
            (lambda fields: fields.__setitem__(3, "2"), "base NMLmsg"),
            (lambda fields: fields.__setitem__(11, "00" * 256), "payload disagrees"),
            (lambda fields: fields.__setitem__(12, "00"), "text disagrees"),
            (lambda fields: fields.__setitem__(13, "01" * 4), "padding disagrees"),
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for index, (mutation, message) in enumerate(mutations):
                path = root / f"mutated-{index}.tsv"
                self.write(path, event_line(1, 1, mutate=mutation))
                with self.assertRaisesRegex(RuntimeError, message):
                    ErrorJournalReader(path).poll()

            path = root / "checksum.tsv"
            line = bytearray(event_line(1, 1))
            line[-2] = ord("0") if line[-2] != ord("0") else ord("1")
            self.write(path, bytes(line))
            with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
                ErrorJournalReader(path).poll()

            path = root / "sequence.tsv"
            self.write(path, event_line(2, 1))
            with self.assertRaisesRegex(RuntimeError, "expected 1, found 2"):
                ErrorJournalReader(path).poll()

            path = root / "header.tsv"
            path.write_bytes(header().replace(b"\t1\t", b"\t2\t", 1))
            with self.assertRaisesRegex(RuntimeError, "header changed"):
                ErrorJournalReader(path).poll()

    def test_every_framing_and_typed_event_field_contract_is_enforced(self):
        mutations = (
            (lambda fields: fields.__setitem__(0, "WRONG"), "marker or schema"),
            (lambda fields: fields.__setitem__(1, "2"), "marker or schema"),
            (lambda fields: fields.__setitem__(2, "0"), "sequence is outside"),
            (
                lambda fields: fields.__setitem__(2, str(1 << 64)),
                "sequence is outside",
            ),
            (
                lambda fields: fields.__setitem__(3, str(1 << 31)),
                "message type is outside",
            ),
            (lambda fields: fields.__setitem__(4, "WRONG"), "classification changed"),
            (lambda fields: fields.__setitem__(5, "warning"), "classification changed"),
            (lambda fields: fields.__setitem__(6, "2"), "known flag"),
            (lambda fields: fields.__setitem__(7, "281"), "object length"),
            (lambda fields: fields.__setitem__(8, "271"), "declared size"),
            (lambda fields: fields.__setitem__(9, "0"), "invented operator metadata"),
            (lambda fields: fields.__setitem__(10, "0"), "invented operator metadata"),
            (lambda fields: fields.__setitem__(11, "gg"), "payload hex"),
            (lambda fields: fields.__setitem__(12, "00"), "text disagrees"),
            (lambda fields: fields.__setitem__(13, "01" * 4), "padding disagrees"),
            (lambda fields: fields.__setitem__(14, fields[14][:-2]), "object length"),
            (lambda fields: fields.__setitem__(15, "3"), "transport state"),
            (lambda fields: fields.__setitem__(16, "2"), "transport state"),
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for index, (mutation, expected) in enumerate(mutations):
                path = root / f"field-{index}.tsv"
                self.write(path, event_line(1, 1, mutate=mutation))
                with self.subTest(field=index), self.assertRaisesRegex(
                    RuntimeError, expected
                ):
                    ErrorJournalReader(path).poll()

            too_few = root / "too-few.tsv"
            self.write(too_few, event_line(1, 1, mutate=lambda fields: fields.pop()))
            with self.assertRaisesRegex(RuntimeError, "17 fields instead of 18"):
                ErrorJournalReader(too_few).poll()

            too_many = root / "too-many.tsv"
            self.write(
                too_many,
                event_line(1, 1, mutate=lambda fields: fields.append("extra")),
            )
            with self.assertRaisesRegex(RuntimeError, "19 fields instead of 18"):
                ErrorJournalReader(too_many).poll()

            non_ascii = root / "non-ascii.tsv"
            non_ascii.write_bytes(b"\xff\n")
            with self.assertRaisesRegex(RuntimeError, "non-ASCII framing"):
                ErrorJournalReader(non_ascii).poll()

    def test_object_layout_operator_metadata_and_integer_widths_are_enforced(self):
        def mutate_object(fields, offset, data):
            object_bytes = bytearray.fromhex(fields[14])
            object_bytes[offset : offset + len(data)] = data
            fields[14] = object_bytes.hex()

        cases = (
            (
                1,
                lambda fields: mutate_object(
                    fields, 0, (2).to_bytes(4, sys.byteorder, signed=True)
                ),
                "base NMLmsg",
            ),
            (
                1,
                lambda fields: mutate_object(
                    fields, 8, (271).to_bytes(8, sys.byteorder, signed=True)
                ),
                "base NMLmsg",
            ),
            (
                11,
                lambda fields: fields.__setitem__(9, str(1 << 31)),
                "serial number is outside",
            ),
            (
                11,
                lambda fields: fields.__setitem__(10, str(-(1 << 31) - 1)),
                "operator id is outside",
            ),
            (
                11,
                lambda fields: fields.__setitem__(9, "0"),
                "operator metadata disagrees",
            ),
            (
                11,
                lambda fields: fields.__setitem__(10, "0"),
                "operator metadata disagrees",
            ),
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for index, (kind, mutation, expected) in enumerate(cases):
                path = root / f"layout-{index}.tsv"
                self.write(path, event_line(1, kind, mutate=mutation))
                with self.subTest(case=index), self.assertRaisesRegex(
                    RuntimeError, expected
                ):
                    ErrorJournalReader(path).poll()

    def test_every_header_field_nonregular_path_and_failed_identity_are_latched(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            original_header = header().rstrip(b"\n").decode("ascii").split("\t")
            for index in range(len(original_header)):
                fields = list(original_header)
                fields[index] += "-changed"
                path = root / f"header-{index}.tsv"
                path.write_text("\t".join(fields) + "\n", encoding="ascii")
                with self.subTest(field=index), self.assertRaisesRegex(
                    RuntimeError, "header changed"
                ):
                    ErrorJournalReader(path).poll()

            directory_path = root / "directory"
            directory_path.mkdir()
            with self.assertRaisesRegex(RuntimeError, "not a regular file"):
                ErrorJournalReader(directory_path).poll()

            target = root / "target.tsv"
            self.write(target, event_line(1, 1))
            link = root / "link.tsv"
            link.symlink_to(target)
            with self.assertRaisesRegex(RuntimeError, "not a regular file"):
                ErrorJournalReader(link).poll()

            corrupt = root / "corrupt.tsv"
            self.write(corrupt, event_line(2, 1))
            reader = ErrorJournalReader(corrupt)
            with self.assertRaisesRegex(RuntimeError, "expected 1, found 2"):
                reader.poll()
            self.assertIsNone(reader.poll())
            original_inode = corrupt.stat().st_ino
            self.write(corrupt, event_line(1, 2))
            self.assertEqual(corrupt.stat().st_ino, original_inode)
            self.assertEqual(reader.poll().message_type, 2)
            replacement = root / "replacement.tsv"
            self.write(replacement, event_line(1, 3))
            os.replace(replacement, corrupt)
            self.assertEqual(reader.poll().message_type, 3)

    def test_invalid_utf8_is_preserved_and_replaced_only_for_display(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "errors.tsv"
            self.write(path, event_line(1, 2, b"before\xffafter"))
            event = ErrorJournalReader(path).poll()
            self.assertEqual(event.text, b"before\xffafter")
            self.assertEqual(event.display_text(), "before\ufffdafter")

    def test_unknown_positive_type_is_preserved_without_inventing_a_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "errors.tsv"
            self.write(path, unknown_event_line(1))
            event = ErrorJournalReader(path).poll()
            self.assertFalse(event.known)
            self.assertEqual(event.message_type, 77)
            self.assertEqual(event.class_name, "UNKNOWN")
            self.assertEqual(event.payload, b"")
            self.assertEqual(len(event.object_bytes), 32)
            self.assertEqual(
                event.display_text(), "Unknown LinuxCNC error-channel message type 77"
            )


if __name__ == "__main__":
    unittest.main()
