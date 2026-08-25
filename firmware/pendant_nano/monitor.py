#!/usr/bin/env python3
from __future__ import annotations

import argparse
import sys
import time

import serial

from pendant_protocol import ProtocolError, parse_packet, signed_int32_delta


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Monitor-only MYST1474 Nano decoder; never commands motion"
    )
    parser.add_argument("--port", default="/dev/ttyUSB0")
    parser.add_argument("--baud", type=int, default=115200)
    args = parser.parse_args()

    print(
        f"MONITOR ONLY: opening {args.port} at {args.baud}; "
        "no LinuxCNC/HAL/Mesa commands are present",
        flush=True,
    )

    previous = None
    with serial.Serial(args.port, args.baud, timeout=1.0) as connection:
        # A classic Nano normally resets when the CH341 asserts DTR on open.
        # Discard no packets: the first valid packet establishes the baseline.
        while True:
            raw = connection.readline()
            if not raw:
                print("TIMEOUT: no Nano packet for 1 second", file=sys.stderr, flush=True)
                previous = None
                continue
            line = raw.decode("ascii", errors="replace").strip()
            if not line:
                continue
            if line.startswith("BOOT,"):
                print(line, flush=True)
                previous = None
                continue
            try:
                packet = parse_packet(line)
            except ProtocolError as error:
                print(f"REJECTED {line!r}: {error}", file=sys.stderr, flush=True)
                previous = None
                continue

            delta = 0 if previous is None else signed_int32_delta(
                packet.detent_count, previous.detent_count
            )
            changed = (
                previous is None
                or delta != 0
                or packet.latest_detent_signal != 0
                or packet.axis != previous.axis
                or packet.multiplier != previous.multiplier
                or packet.deadman_held != previous.deadman_held
                or packet.estop_pressed != previous.estop_pressed
                or packet.selector_valid != previous.selector_valid
                or packet.quadrature_errors != previous.quadrature_errors
            )
            if changed:
                print(
                    f"seq={packet.sequence} transitions={packet.transition_count:+d} "
                    f"detents={packet.detent_count:+d} detent_delta={delta:+d} "
                    f"latest_signal={packet.latest_detent_signal:+d} "
                    f"errors={packet.quadrature_errors} "
                    f"axis={packet.axis} multiplier={packet.multiplier} "
                    f"deadman={'HELD' if packet.deadman_held else 'RELEASED'} "
                    f"estop={'PRESSED/OPEN' if packet.estop_pressed else 'RELEASED/CLOSED'} "
                    f"selectors={'VALID' if packet.selector_valid else 'INVALID'}",
                    flush=True,
                )
            previous = packet


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("\nMONITOR STOPPED", file=sys.stderr)
        raise SystemExit(130)
