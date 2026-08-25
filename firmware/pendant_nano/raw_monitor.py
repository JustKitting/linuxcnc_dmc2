#!/usr/bin/env python3
"""Print raw Nano pin reports. This program cannot command CNC motion."""

import argparse

import serial


parser = argparse.ArgumentParser()
parser.add_argument("--port", default="/dev/ttyUSB0")
parser.add_argument("--baud", type=int, default=115200)
args = parser.parse_args()

print(f"RAW MONITOR ONLY: {args.port} at {args.baud}", flush=True)
with serial.Serial(args.port, args.baud, timeout=1.0) as connection:
    while True:
        raw = connection.readline()
        if raw:
            print(raw.decode("ascii", errors="replace").strip(), flush=True)
        else:
            print("TIMEOUT: no raw packet", flush=True)
