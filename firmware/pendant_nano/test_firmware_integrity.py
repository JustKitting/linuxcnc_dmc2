from __future__ import annotations

import re
import unittest
from pathlib import Path


SOURCE = Path(__file__).parent / "pendant_decoder" / "pendant_decoder.ino"


class FirmwareIntegrityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = SOURCE.read_text(encoding="utf-8")

    def test_user_observed_pin_map_is_exact(self):
        expected = (
            "constexpr uint8_t PIN_ENCODER_A = 2;",
            "constexpr uint8_t PIN_ENCODER_B = 3;",
            "constexpr uint8_t AXIS_PINS[] = {4, 5, 6, 7, 8};",
            "constexpr uint8_t MULTIPLIER_PINS[] = {9, 10, 11};",
            "constexpr uint8_t PIN_ESTOP = 12;",
        )
        for declaration in expected:
            self.assertIn(declaration, self.source)

    def test_measured_cw_and_ccw_sequences_are_exact_inverses(self):
        match = re.search(
            r"QUADRATURE_LUT\[16\]\s*=\s*\{(?P<body>.*?)\};",
            self.source,
            re.DOTALL,
        )
        self.assertIsNotNone(match)
        lut = [
            int(token)
            for token in re.findall(r"(?<![A-Za-z0-9_])[+-]?\d+", match["body"])
        ]
        self.assertEqual(len(lut), 16)

        def transitions(states):
            return sum(lut[(previous << 2) | current] for previous, current in zip(states, states[1:]))

        clockwise = (0b00, 0b10, 0b11, 0b01, 0b00)
        counterclockwise = tuple(reversed(clockwise))
        self.assertEqual(transitions(clockwise), 4)
        self.assertEqual(transitions(counterclockwise), -4)

    def test_selector_transition_is_immediately_non_commanding(self):
        required = (
            "selectorSettling = true;",
            "discardPendingWheelMotion();",
            "SelectorState publishedSelectorState()",
            "return {SELECT_INVALID, SELECT_INVALID, false};",
            "now - selectorCandidateSince >= SELECTOR_DEBOUNCE_MS",
        )
        for token in required:
            self.assertIn(token, self.source)

    def test_estop_edge_discards_wheel_motion_before_snapshot(self):
        edge = self.source.index("if (estopPressed != previousEstopPressed)")
        discard = self.source.index("discardPendingWheelMotion();", edge)
        snapshot = self.source.index("detentSnapshot = detentCount;", edge)
        self.assertLess(edge, discard)
        self.assertLess(discard, snapshot)

    def test_selector_acceptance_discards_debounce_interval_motion(self):
        settle = self.source.index("if (selectorSettling &&")
        accepted = self.source.index("selectorStable = selectorCandidate;", settle)
        discard = self.source.index("discardPendingWheelMotion();", accepted)
        self.assertLess(accepted, discard)

    def test_one_slot_signal_is_consumed_and_cleared_each_report(self):
        snapshot = self.source.index("latestDetentSnapshot = latestDetentSignal;")
        clear = self.source.index("latestDetentSignal = 0;", snapshot)
        publish = self.source.index("Serial.print(latestDetentSnapshot);", clear)
        self.assertLess(snapshot, clear)
        self.assertLess(clear, publish)

    def test_worst_case_packet_fits_one_20_ms_serial_budget(self):
        worst_case = (
            "P3,4294967295,4294967295,-2147483648,-2147483648,"
            "4294967295,-1,X,X100,1,1,1\r\n"
        )
        bytes_per_interval = 115200 / 10 * 0.020
        self.assertLess(len(worst_case.encode("ascii")), bytes_per_interval)


if __name__ == "__main__":
    unittest.main()
