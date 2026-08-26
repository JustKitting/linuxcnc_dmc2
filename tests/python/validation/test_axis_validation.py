from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from dmc2_validation.axis import validate_axis_error_reader_handoff


VALID_BODY = b"""\
def error_task(self):
    error = e.poll()
    error = e.poll()
e = linuxcnc.error_channel()
exec(compile(open(rcfile, \"rb\").read(), rcfile, 'exec'))
live_plotter.error_task()
"""


class AxisValidationTests(unittest.TestCase):
    def sources(self, directory: Path, body: bytes = VALID_BODY):
        pinned = directory / "pinned-axis"
        installed = directory / "installed-axis"
        pinned.write_bytes(b"#!/usr/bin/env python3\n" + body)
        installed.write_bytes(b"#! /usr/bin/python3\n" + body)
        return pinned, installed

    def test_exact_body_with_build_time_shebang_difference_is_accepted(self):
        with tempfile.TemporaryDirectory() as directory:
            pinned, installed = self.sources(Path(directory))
            self.assertIn(
                "transfers error-reader ownership before its first poll",
                validate_axis_error_reader_handoff(
                    pinned_axis=pinned,
                    installed_axis=installed,
                ),
            )

    def test_any_installed_body_difference_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pinned, installed = self.sources(root)
            installed.write_bytes(installed.read_bytes() + b"# changed\n")
            with self.assertRaisesRegex(AssertionError, "differs from pinned"):
                validate_axis_error_reader_handoff(
                    pinned_axis=pinned,
                    installed_axis=installed,
                )

    def test_every_required_stock_axis_handoff_fact_is_enforced(self):
        mutations = (
            (
                lambda body: body.replace(b"e = linuxcnc.error_channel()", b""),
                "constructor",
            ),
            (lambda body: body.replace(b"error = e.poll()", b"", 1), "polling"),
            (
                lambda body: body.replace(
                    b"exec(compile(open(rcfile, \"rb\").read(), rcfile, 'exec'))",
                    b"",
                ),
                "USER_COMMAND_FILE",
            ),
            (
                lambda body: body.replace(b"live_plotter.error_task()", b""),
                "first error-poll",
            ),
            (
                lambda body: body.replace(
                    b"exec(compile(open(rcfile, \"rb\").read(), rcfile, 'exec'))\n"
                    b"live_plotter.error_task()",
                    b"live_plotter.error_task()\n"
                    b"exec(compile(open(rcfile, \"rb\").read(), rcfile, 'exec'))",
                ),
                "after constructing and before polling",
            ),
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for index, (mutation, expected) in enumerate(mutations):
                body = mutation(VALID_BODY)
                pinned, installed = self.sources(root, body)
                with self.subTest(case=index), self.assertRaisesRegex(
                    AssertionError, expected
                ):
                    validate_axis_error_reader_handoff(
                        pinned_axis=pinned,
                        installed_axis=installed,
                    )

    def test_missing_shebang_and_unreadable_source_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pinned, installed = self.sources(root)
            pinned.write_bytes(VALID_BODY)
            with self.assertRaisesRegex(AssertionError, "no executable shebang"):
                validate_axis_error_reader_handoff(
                    pinned_axis=pinned,
                    installed_axis=installed,
                )
            with self.assertRaisesRegex(AssertionError, "cannot read AXIS source"):
                validate_axis_error_reader_handoff(
                    pinned_axis=root / "missing",
                    installed_axis=installed,
                )


if __name__ == "__main__":
    unittest.main()
