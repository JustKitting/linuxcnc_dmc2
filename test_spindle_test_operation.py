from __future__ import annotations

import re
import unittest
from pathlib import Path

from validate_offline import (
    ROOT,
    executable_gcode_text,
    validate_spindle_test_operation,
)


PROGRAM = ROOT / "live" / "nc_files" / "dmc2_spindle_test.ngc"
MACHINE_HAL = ROOT / "live" / "machine.hal"


class SpindleTestOperationTests(unittest.TestCase):
    def test_operation_is_parameterized_instead_of_fixed_at_one_speed(self):
        program = executable_gcode_text(PROGRAM)
        self.assertIn("#<test_rpm> = #1", program)
        self.assertIn("S#<test_rpm> M3", program)
        self.assertIsNone(re.search(r"(?i)\bS\s*[-+]?\d", program))

    def test_operation_uses_live_feedback_and_then_stops(self):
        program = executable_gcode_text(PROGRAM)
        self.assertIn("M66 P3 L3 Q#<transition_timeout_seconds>", program)
        self.assertIn("M66 P2 L3 Q#<transition_timeout_seconds>", program)
        self.assertGreaterEqual(
            program.count("M66 P3 L4 Q#<transition_timeout_seconds>"),
            2,
        )
        self.assertGreaterEqual(program.upper().count("M5"), 4)

        hal = MACHINE_HAL.read_text(encoding="utf-8")
        self.assertIn(
            "h100-spindle.at-speed => spindle.0.at-speed motion.digital-in-02",
            hal,
        )
        self.assertIn(
            "h100-spindle.running => motion.digital-in-03",
            hal,
        )

    def test_operation_contains_no_axis_motion_or_reverse(self):
        program = executable_gcode_text(PROGRAM)
        self.assertIsNone(
            re.search(r"(?im)^\s*G(?:0|1|2|3|38(?:\.\d+)?)\b", program)
        )
        self.assertIsNone(re.search(r"(?i)\bM4\b", program))

    def test_offline_validator_accepts_the_complete_operation(self):
        self.assertIn("takes RPM as #1", validate_spindle_test_operation())


if __name__ == "__main__":
    unittest.main()
