from __future__ import annotations

import json
import subprocess
import sys
import unittest
from pathlib import Path

from nano_hal_bridge import (
    MAX_SERIAL_LINE_BYTES,
    BridgeState,
    accept_line,
    publish,
)

PROJECT_DIR = Path(__file__).resolve().parent
BRIDGE = PROJECT_DIR / "nano_hal_bridge.py"


def packet(
    sequence: int,
    *,
    signal: int = 0,
    axis: str = "X",
    multiplier: str = "X1",
    deadman: int = 0,
    estop: int = 0,
    valid: int = 1,
    errors: int = 0,
) -> str:
    return (
        f"P3,{sequence},{(sequence * 20) & 0xFFFFFFFF},0,0,{errors},{signal},"
        f"{axis},{multiplier},{deadman},{estop},{valid}"
    )


class BridgeStateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.state = BridgeState(0.100)

    def establish(self, sequence: int = 1, **kwargs) -> None:
        accept_line(self.state, packet(sequence, **kwargs), sequence * 0.020)

    def test_startup_is_fail_closed(self):
        snapshot = self.state.snapshot
        self.assertFalse(snapshot.connected)
        self.assertTrue(snapshot.serial_fault)
        self.assertTrue(snapshot.estop_pressed)
        self.assertEqual(snapshot.latest_detent, 0)

    def test_first_valid_packet_establishes_baseline_and_discards_detent(self):
        self.establish(signal=1, deadman=1)
        snapshot = self.state.snapshot
        self.assertTrue(snapshot.connected)
        self.assertFalse(snapshot.serial_fault)
        self.assertTrue(snapshot.deadman_held)
        self.assertEqual(snapshot.latest_detent, 0)

    def test_subsequent_packet_exposes_only_its_latest_detent(self):
        self.establish()
        accept_line(self.state, packet(2, signal=-1), 0.040)
        self.assertEqual(self.state.snapshot.latest_detent, -1)
        accept_line(self.state, packet(3, signal=0), 0.060)
        self.assertEqual(self.state.snapshot.latest_detent, 0)

    def test_packet_gap_is_counted_but_never_replayed_or_accumulated(self):
        self.establish()
        accept_line(self.state, packet(50, signal=1), 1.000)
        self.assertEqual(self.state.snapshot.dropped_packets, 48)
        self.assertEqual(self.state.snapshot.latest_detent, 1)

    def test_timeout_returns_to_fail_closed_state_once(self):
        self.establish()
        self.assertTrue(self.state.check_timeout(0.121))
        self.assertFalse(self.state.snapshot.connected)
        self.assertTrue(self.state.snapshot.estop_pressed)
        self.assertTrue(self.state.snapshot.serial_fault)
        self.assertEqual(self.state.snapshot.timeouts, 1)
        self.assertFalse(self.state.check_timeout(0.500))
        self.assertEqual(self.state.snapshot.timeouts, 1)

    def test_packet_after_timeout_is_a_fresh_non_commanding_baseline(self):
        self.establish()
        self.state.check_timeout(0.121)
        accept_line(self.state, packet(80, signal=-1), 0.140)
        self.assertTrue(self.state.snapshot.connected)
        self.assertEqual(self.state.snapshot.latest_detent, 0)

    def test_protocol_error_fails_closed_and_requires_fresh_baseline(self):
        self.establish()
        accept_line(self.state, "not-a-packet", 0.040)
        self.assertFalse(self.state.snapshot.connected)
        self.assertEqual(self.state.snapshot.protocol_errors, 1)
        accept_line(self.state, packet(2, signal=1), 0.060)
        self.assertEqual(self.state.snapshot.latest_detent, 0)

    def test_repeated_sequence_fails_closed(self):
        self.establish(sequence=7)
        accept_line(self.state, packet(7, signal=1), 0.160)
        self.assertTrue(self.state.snapshot.serial_fault)
        self.assertFalse(self.state.snapshot.connected)
        self.assertEqual(self.state.snapshot.protocol_errors, 1)

    def test_unsigned_sequence_wrap_is_accepted(self):
        accept_line(self.state, packet(0xFFFFFFFF), 1.0)
        accept_line(self.state, packet(0, signal=1), 1.020)
        self.assertTrue(self.state.snapshot.connected)
        self.assertEqual(self.state.snapshot.latest_detent, 1)

    def test_quadrature_change_is_sticky_and_blocks_detents(self):
        self.establish(errors=3)
        accept_line(self.state, packet(2, signal=1, errors=4), 0.040)
        self.assertTrue(self.state.snapshot.quadrature_fault)
        self.assertFalse(self.state.snapshot.link_healthy)
        self.assertEqual(self.state.snapshot.latest_detent, 0)
        accept_line(self.state, packet(3, signal=-1, errors=4), 0.060)
        self.assertTrue(self.state.snapshot.quadrature_fault)
        self.assertEqual(self.state.snapshot.latest_detent, 0)

    def test_boot_marker_clears_prior_quadrature_fault_and_fails_closed(self):
        self.establish(errors=3)
        accept_line(self.state, packet(2, errors=4), 0.040)
        accept_line(self.state, "BOOT,P3,MYST1474-001,MONITOR_ONLY", 0.050)
        self.assertFalse(self.state.snapshot.quadrature_fault)
        self.assertTrue(self.state.snapshot.serial_fault)
        self.assertTrue(self.state.snapshot.estop_pressed)

    def test_malformed_boot_marker_is_a_protocol_fault(self):
        self.establish()
        accept_line(self.state, "BOOT,P3,WRONG,MONITOR_ONLY", 0.040)
        self.assertTrue(self.state.snapshot.serial_fault)
        self.assertEqual(self.state.snapshot.protocol_errors, 1)

    def test_out_of_range_wire_integer_is_a_protocol_fault(self):
        self.establish()
        accept_line(
            self.state,
            "P3,2,40,2147483648,0,0,0,X,X1,0,0,1",
            0.040,
        )
        self.assertTrue(self.state.snapshot.serial_fault)
        self.assertEqual(self.state.snapshot.protocol_errors, 1)

    def test_axis_and_multiplier_codes_preserve_off_and_invalid(self):
        self.establish(axis="N", multiplier="X1", valid=0, deadman=1)
        self.assertEqual(self.state.snapshot.axis_code, -1)
        self.assertEqual(self.state.snapshot.multiplier_code, 1)
        accept_line(
            self.state,
            packet(2, axis="I", multiplier="I", valid=0),
            0.040,
        )
        self.assertEqual(self.state.snapshot.axis_code, -2)
        self.assertEqual(self.state.snapshot.multiplier_code, -1)

    def test_estop_makes_control_unhealthy_without_hiding_connection(self):
        self.establish(estop=1)
        self.assertTrue(self.state.snapshot.connected)
        self.assertTrue(self.state.snapshot.estop_pressed)
        self.assertFalse(self.state.snapshot.link_healthy)

    def test_publish_brackets_every_snapshot_with_even_generation(self):
        self.establish(sequence=27, axis="Y", multiplier="X100", deadman=1)
        component = {}
        publish(component, self.state.snapshot, 0.0)
        self.assertEqual(component["snapshot-generation"], 54)
        self.assertFalse(component["snapshot-generation"] & 1)
        self.assertEqual(component["sequence"], 27)

    def test_hal_u32_diagnostic_counters_are_explicitly_masked(self):
        self.establish()
        self.state.snapshot = self.state.snapshot.__class__(
            **{
                **self.state.snapshot.__dict__,
                "protocol_errors": (1 << 40) + 7,
                "dropped_packets": (1 << 40) + 8,
                "timeouts": (1 << 40) + 9,
            }
        )
        component = {}
        publish(component, self.state.snapshot, 0.0)
        self.assertEqual(component["protocol-errors"], 7)
        self.assertEqual(component["dropped-packets"], 8)
        self.assertEqual(component["timeouts"], 9)

    def test_serial_line_limit_has_headroom_above_worst_valid_packet(self):
        worst_valid = packet(
            0xFFFFFFFF,
            signal=-1,
            axis="X",
            multiplier="X100",
            deadman=1,
            estop=1,
            valid=1,
            errors=0xFFFFFFFF,
        )
        self.assertLess(len(worst_valid.encode("ascii")), MAX_SERIAL_LINE_BYTES)


class OfflineCliTests(unittest.TestCase):
    def test_validate_mode_uses_neither_serial_device_nor_hal_runtime(self):
        completed = subprocess.run(
            [sys.executable, str(BRIDGE), "--validate"],
            cwd=PROJECT_DIR,
            text=True,
            capture_output=True,
            check=True,
        )
        result = json.loads(completed.stdout)
        self.assertTrue(result["connected"])
        self.assertTrue(result["deadman_held"])
        self.assertEqual(result["latest_detent"], 1)
        self.assertEqual(result["axis_code"], 0)
        self.assertEqual(result["multiplier_code"], 1)


if __name__ == "__main__":
    unittest.main()
