#!/usr/bin/env python3
"""Expose the Arduino Nano P3 pendant stream as read-only LinuxCNC HAL pins.

This component never loads or addresses the Mesa card and has no motion,
spindle, output, or probe-power pins.  It is deliberately limited to decoding
the Nano's USB serial status stream.  Every communication fault returns the
published state to a fail-closed baseline.
"""

from __future__ import annotations

import argparse
import json
import signal
import sys
import time
from dataclasses import asdict, dataclass, replace
from pathlib import Path
from typing import Iterable

PROJECT_ROOT = Path(__file__).resolve().parents[1]
PENDANT_PROTOCOL_DIR = PROJECT_ROOT / "pendant_nano"
if str(PENDANT_PROTOCOL_DIR) not in sys.path:
    sys.path.insert(0, str(PENDANT_PROTOCOL_DIR))

from pendant_protocol import PendantPacket, ProtocolError, parse_packet


COMPONENT_NAME = "dmc2-pendant"
BOOT_MARKER = "BOOT,P3,MYST1474-001,MONITOR_ONLY"
DEFAULT_PORT = "/dev/ttyUSB0"
DEFAULT_BAUD = 115200
DEFAULT_PACKET_TIMEOUT_SECONDS = 0.100
DEFAULT_SERIAL_READ_TIMEOUT_SECONDS = 0.025
MAX_SERIAL_LINE_BYTES = 128
OVERLONG_LINE_MARKER = "INVALID,OVERLONG-SERIAL-LINE"

AXIS_CODES = {"X": 0, "Y": 1, "Z": 2, "4": 3, "5": 4, "N": -1, "I": -2}
MULTIPLIER_CODES = {"X1": 1, "X10": 10, "X100": 100, "N": 0, "I": -1}
U32_MAX = (1 << 32) - 1
S32_MIN = -(1 << 31)
S32_MAX = (1 << 31) - 1


@dataclass(frozen=True)
class BridgeSnapshot:
    connected: bool = False
    serial_fault: bool = True
    quadrature_fault: bool = False
    link_healthy: bool = False
    heartbeat: bool = False
    estop_pressed: bool = True
    deadman_held: bool = False
    selector_valid: bool = False
    axis: str = "I"
    multiplier: str = "I"
    latest_detent: int = 0
    detent_count: int = 0
    transition_count: int = 0
    quadrature_errors: int = 0
    sequence: int = 0
    milliseconds: int = 0
    protocol_errors: int = 0
    dropped_packets: int = 0
    timeouts: int = 0

    @property
    def axis_code(self) -> int:
        return AXIS_CODES[self.axis]

    @property
    def multiplier_code(self) -> int:
        return MULTIPLIER_CODES[self.multiplier]


class BridgeState:
    """Pure packet/fault state machine, independent of serial and HAL."""

    def __init__(self, packet_timeout_seconds: float) -> None:
        if packet_timeout_seconds <= 0:
            raise ValueError("packet timeout must be positive")
        self.packet_timeout_seconds = packet_timeout_seconds
        self.snapshot = BridgeSnapshot()
        self.last_packet_at: float | None = None
        self.previous_sequence: int | None = None
        self.previous_quadrature_errors: int | None = None
        self._baseline_required = True

    def _safe_snapshot(self, *, serial_fault: bool) -> BridgeSnapshot:
        old = self.snapshot
        return BridgeSnapshot(
            connected=False,
            serial_fault=serial_fault,
            quadrature_fault=old.quadrature_fault,
            heartbeat=old.heartbeat,
            estop_pressed=True,
            protocol_errors=old.protocol_errors,
            dropped_packets=old.dropped_packets,
            timeouts=old.timeouts,
            quadrature_errors=old.quadrature_errors,
        )

    def reset_for_boot(self) -> None:
        """Treat a Nano boot marker as a fresh, non-commanding baseline."""
        old = self.snapshot
        self.snapshot = BridgeSnapshot(
            connected=False,
            serial_fault=True,
            heartbeat=old.heartbeat,
            estop_pressed=True,
            protocol_errors=old.protocol_errors,
            dropped_packets=old.dropped_packets,
            timeouts=old.timeouts,
        )
        self.last_packet_at = None
        self.previous_sequence = None
        self.previous_quadrature_errors = None
        self._baseline_required = True

    def note_serial_fault(self) -> None:
        self.snapshot = self._safe_snapshot(serial_fault=True)
        self.last_packet_at = None
        self.previous_sequence = None
        self.previous_quadrature_errors = None
        self._baseline_required = True

    def note_protocol_error(self) -> None:
        self.snapshot = replace(
            self._safe_snapshot(serial_fault=True),
            protocol_errors=self.snapshot.protocol_errors + 1,
        )
        self.last_packet_at = None
        self.previous_sequence = None
        self.previous_quadrature_errors = None
        self._baseline_required = True

    def accept(self, packet: PendantPacket, now: float) -> BridgeSnapshot:
        if self.previous_sequence is not None:
            sequence_delta = (packet.sequence - self.previous_sequence) & 0xFFFFFFFF
            if sequence_delta == 0 or sequence_delta > 0x7FFFFFFF:
                self.note_protocol_error()
                return self.snapshot
            if sequence_delta > 1:
                self.snapshot = replace(
                    self.snapshot,
                    dropped_packets=self.snapshot.dropped_packets + sequence_delta - 1,
                )

        first_packet = self._baseline_required
        quadrature_fault = self.snapshot.quadrature_fault
        if (
            self.previous_quadrature_errors is not None
            and packet.quadrature_errors != self.previous_quadrature_errors
        ):
            quadrature_fault = True

        latest_detent = 0 if first_packet or quadrature_fault else packet.latest_detent_signal
        serial_fault = False
        link_healthy = not serial_fault and not quadrature_fault and not packet.estop_pressed

        self.snapshot = BridgeSnapshot(
            connected=True,
            serial_fault=serial_fault,
            quadrature_fault=quadrature_fault,
            link_healthy=link_healthy,
            heartbeat=not self.snapshot.heartbeat,
            estop_pressed=packet.estop_pressed,
            deadman_held=packet.deadman_held,
            selector_valid=packet.selector_valid,
            axis=packet.axis,
            multiplier=packet.multiplier,
            latest_detent=latest_detent,
            detent_count=packet.detent_count,
            transition_count=packet.transition_count,
            quadrature_errors=packet.quadrature_errors,
            sequence=packet.sequence,
            milliseconds=packet.milliseconds,
            protocol_errors=self.snapshot.protocol_errors,
            dropped_packets=self.snapshot.dropped_packets,
            timeouts=self.snapshot.timeouts,
        )
        self.last_packet_at = now
        self.previous_sequence = packet.sequence
        self.previous_quadrature_errors = packet.quadrature_errors
        self._baseline_required = False
        return self.snapshot

    def check_timeout(self, now: float) -> bool:
        if self.last_packet_at is None:
            return False
        if now - self.last_packet_at <= self.packet_timeout_seconds:
            return False
        self.snapshot = replace(
            self._safe_snapshot(serial_fault=True),
            timeouts=self.snapshot.timeouts + 1,
        )
        self.last_packet_at = None
        self.previous_sequence = None
        self.previous_quadrature_errors = None
        self._baseline_required = True
        return True

    def packet_age_ms(self, now: float) -> float:
        if self.last_packet_at is None:
            return -1.0
        return max(0.0, (now - self.last_packet_at) * 1000.0)


def accept_line(state: BridgeState, line: str, now: float) -> BridgeSnapshot:
    stripped = line.strip()
    if not stripped:
        return state.snapshot
    if stripped == BOOT_MARKER:
        state.reset_for_boot()
        return state.snapshot
    if stripped.startswith("BOOT,"):
        state.note_protocol_error()
        return state.snapshot
    try:
        packet = parse_packet(stripped)
    except ProtocolError:
        state.note_protocol_error()
        return state.snapshot
    unsigned_values = (
        packet.sequence,
        packet.milliseconds,
        packet.quadrature_errors,
    )
    signed_values = (packet.detent_count, packet.transition_count)
    if not all(0 <= value <= U32_MAX for value in unsigned_values):
        state.note_protocol_error()
        return state.snapshot
    if not all(S32_MIN <= value <= S32_MAX for value in signed_values):
        state.note_protocol_error()
        return state.snapshot
    return state.accept(packet, now)


HAL_PINS = (
    ("snapshot-generation", "u32"),
    ("connected", "bit"),
    ("serial-fault", "bit"),
    ("quadrature-fault", "bit"),
    ("link-healthy", "bit"),
    ("heartbeat", "bit"),
    ("estop-pressed", "bit"),
    ("deadman-held", "bit"),
    ("selector-valid", "bit"),
    ("axis-x", "bit"),
    ("axis-y", "bit"),
    ("axis-z", "bit"),
    ("axis-4", "bit"),
    ("axis-5", "bit"),
    ("axis-off", "bit"),
    ("axis-invalid", "bit"),
    ("multiplier-x1", "bit"),
    ("multiplier-x10", "bit"),
    ("multiplier-x100", "bit"),
    ("multiplier-off", "bit"),
    ("multiplier-invalid", "bit"),
    ("axis-code", "s32"),
    ("multiplier-code", "s32"),
    ("latest-detent", "s32"),
    ("detent-count", "s32"),
    ("transition-count", "s32"),
    ("quadrature-errors", "u32"),
    ("sequence", "u32"),
    ("milliseconds", "u32"),
    ("protocol-errors", "u32"),
    ("dropped-packets", "u32"),
    ("timeouts", "u32"),
    ("packet-age-ms", "float"),
)


def create_hal_component(component_name: str):
    import hal

    type_by_name = {
        "bit": hal.HAL_BIT,
        "float": hal.HAL_FLOAT,
        "s32": hal.HAL_S32,
        "u32": hal.HAL_U32,
    }
    component = hal.component(component_name)
    for pin_name, type_name in HAL_PINS:
        component.newpin(pin_name, type_by_name[type_name], hal.HAL_OUT)
    component.ready()
    return component


def publish(component, snapshot: BridgeSnapshot, packet_age_ms: float) -> None:
    # Userspace HAL pin updates are individual writes, not an atomic struct.
    # Mark the snapshot busy before touching any field and stable only after
    # every field is complete so the 1 kHz consumer can reject torn reads.
    generation_base = (snapshot.sequence & 0x7FFFFFFF) << 1
    component["snapshot-generation"] = generation_base | 1
    values = {
        "connected": snapshot.connected,
        "serial-fault": snapshot.serial_fault,
        "quadrature-fault": snapshot.quadrature_fault,
        "link-healthy": snapshot.link_healthy,
        "heartbeat": snapshot.heartbeat,
        "estop-pressed": snapshot.estop_pressed,
        "deadman-held": snapshot.deadman_held,
        "selector-valid": snapshot.selector_valid,
        "axis-x": snapshot.axis == "X",
        "axis-y": snapshot.axis == "Y",
        "axis-z": snapshot.axis == "Z",
        "axis-4": snapshot.axis == "4",
        "axis-5": snapshot.axis == "5",
        "axis-off": snapshot.axis == "N",
        "axis-invalid": snapshot.axis == "I",
        "multiplier-x1": snapshot.multiplier == "X1",
        "multiplier-x10": snapshot.multiplier == "X10",
        "multiplier-x100": snapshot.multiplier == "X100",
        "multiplier-off": snapshot.multiplier == "N",
        "multiplier-invalid": snapshot.multiplier == "I",
        "axis-code": snapshot.axis_code,
        "multiplier-code": snapshot.multiplier_code,
        "latest-detent": snapshot.latest_detent,
        "detent-count": snapshot.detent_count,
        "transition-count": snapshot.transition_count,
        "quadrature-errors": snapshot.quadrature_errors & 0xFFFFFFFF,
        "sequence": snapshot.sequence & 0xFFFFFFFF,
        "milliseconds": snapshot.milliseconds & 0xFFFFFFFF,
        "protocol-errors": snapshot.protocol_errors & 0xFFFFFFFF,
        "dropped-packets": snapshot.dropped_packets & 0xFFFFFFFF,
        "timeouts": snapshot.timeouts & 0xFFFFFFFF,
        "packet-age-ms": packet_age_ms,
    }
    for pin_name, value in values.items():
        component[pin_name] = value
    component["snapshot-generation"] = generation_base


def replay_lines(path: Path) -> list[str]:
    lines = [line.strip() for line in path.read_text(encoding="ascii").splitlines()]
    return [line for line in lines if line and not line.startswith("#")]


def validate_only() -> int:
    state = BridgeState(DEFAULT_PACKET_TIMEOUT_SECONDS)
    accept_line(state, BOOT_MARKER, 1.0)
    accept_line(state, "P3,1,20,0,0,0,0,X,X1,0,0,1", 1.020)
    accept_line(state, "P3,2,40,1,4,0,1,X,X1,1,0,1", 1.040)
    result = asdict(state.snapshot)
    result["axis_code"] = state.snapshot.axis_code
    result["multiplier_code"] = state.snapshot.multiplier_code
    result["packet_age_ms"] = state.packet_age_ms(1.040)
    print(json.dumps(result, sort_keys=True))
    return 0


def live_serial_lines(port: str, baud: int) -> Iterable[str]:
    import serial

    with serial.Serial(
        port,
        baud,
        timeout=DEFAULT_SERIAL_READ_TIMEOUT_SECONDS,
    ) as connection:
        while True:
            # A disconnected or corrupt device must not make readline build an
            # unbounded bytes object while waiting for a newline.
            raw = connection.readline(MAX_SERIAL_LINE_BYTES + 1)
            if not raw:
                yield ""
            elif len(raw) > MAX_SERIAL_LINE_BYTES:
                yield OVERLONG_LINE_MARKER
            else:
                yield raw.decode("ascii", errors="replace")


def run_component(args: argparse.Namespace) -> int:
    state = BridgeState(args.packet_timeout_ms / 1000.0)
    component = create_hal_component(args.component)
    publish(component, state.snapshot, -1.0)

    stopping = False

    def request_stop(_signum, _frame) -> None:
        nonlocal stopping
        stopping = True

    signal.signal(signal.SIGINT, request_stop)
    signal.signal(signal.SIGTERM, request_stop)

    try:
        if args.replay is not None:
            source_lines = replay_lines(args.replay)
            if not source_lines:
                raise RuntimeError(f"replay file contains no packets: {args.replay}")
            for line in source_lines:
                if stopping:
                    break
                now = time.monotonic()
                accept_line(state, line, now)
                publish(component, state.snapshot, state.packet_age_ms(now))
                time.sleep(args.replay_period_ms / 1000.0)

            while not stopping and args.replay_idle:
                snapshot = state.snapshot
                idle_packet = PendantPacket(
                    sequence=(snapshot.sequence + 1) & U32_MAX,
                    milliseconds=(
                        snapshot.milliseconds + round(args.replay_period_ms)
                    ) & U32_MAX,
                    detent_count=snapshot.detent_count,
                    transition_count=snapshot.transition_count,
                    quadrature_errors=snapshot.quadrature_errors,
                    latest_detent_signal=0,
                    axis=snapshot.axis,
                    multiplier=snapshot.multiplier,
                    deadman_held=snapshot.deadman_held,
                    estop_pressed=snapshot.estop_pressed,
                    selector_valid=snapshot.selector_valid,
                )
                now = time.monotonic()
                state.accept(idle_packet, now)
                publish(component, state.snapshot, state.packet_age_ms(now))
                time.sleep(args.replay_period_ms / 1000.0)

            while not stopping and not args.replay_once and not args.replay_idle:
                now = time.monotonic()
                state.check_timeout(now)
                publish(component, state.snapshot, state.packet_age_ms(now))
                time.sleep(DEFAULT_SERIAL_READ_TIMEOUT_SECONDS)
            return 0

        while not stopping:
            try:
                for line in live_serial_lines(args.port, args.baud):
                    if stopping:
                        break
                    now = time.monotonic()
                    if line:
                        accept_line(state, line, now)
                    else:
                        state.check_timeout(now)
                    publish(component, state.snapshot, state.packet_age_ms(now))
            except (OSError, RuntimeError) as error:
                state.note_serial_fault()
                publish(component, state.snapshot, -1.0)
                print(f"{args.component}: serial unavailable: {error}", file=sys.stderr)
                deadline = time.monotonic() + 1.0
                while not stopping and time.monotonic() < deadline:
                    time.sleep(0.025)
        return 0
    finally:
        state.note_serial_fault()
        try:
            publish(component, state.snapshot, -1.0)
        finally:
            component.exit()


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--component", default=COMPONENT_NAME)
    parser.add_argument("--port", default=DEFAULT_PORT)
    parser.add_argument("--baud", type=int, default=DEFAULT_BAUD)
    parser.add_argument(
        "--packet-timeout-ms",
        type=float,
        default=DEFAULT_PACKET_TIMEOUT_SECONDS * 1000.0,
    )
    parser.add_argument("--replay", type=Path)
    parser.add_argument(
        "--replay-idle",
        action="store_true",
        help="after replay, emit monotonic zero-detent idle packets (offline only)",
    )
    parser.add_argument("--replay-once", action="store_true")
    parser.add_argument("--replay-period-ms", type=float, default=20.0)
    parser.add_argument("--validate", action="store_true")
    args = parser.parse_args(argv)
    if args.packet_timeout_ms <= 0:
        parser.error("--packet-timeout-ms must be positive")
    if args.replay_period_ms <= 0:
        parser.error("--replay-period-ms must be positive")
    if args.replay_idle and args.replay is None:
        parser.error("--replay-idle requires --replay")
    if args.replay_once and args.replay is None:
        parser.error("--replay-once requires --replay")
    if args.replay_idle and args.replay_once:
        parser.error("--replay-idle and --replay-once are mutually exclusive")
    return args


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if args.validate:
        return validate_only()
    return run_component(args)


if __name__ == "__main__":
    raise SystemExit(main())
