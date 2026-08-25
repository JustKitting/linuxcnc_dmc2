# MYST1474-001 Nano pendant interface

This directory contains the measured pendant mapping, the Nano decoder, and a
monitor that cannot command the CNC.

## Current verified wiring

The pendant is soldered to the Arduino Nano and the following Nano signals were
measured live:

| Nano pin | Wire | Measured function |
|---:|---|---|
| D2 | green | encoder A |
| D3 | white | encoder B |
| D4 | yellow | X selector |
| D5 | yellow/black | Y selector |
| D6 | brown | Z selector |
| D7 | brown/black | axis 4 selector |
| D8 | pink | axis 5 selector |
| D9 | grey | x1 selector |
| D10 | grey/black | x10 selector |
| D11 | orange | x100 selector |
| D12 | blue | E-stop: LOW released, HIGH pressed/open |

The detailed observations, including the side-button topology and encoder
sequences, are in [LIVE_MAPPING.md](LIVE_MAPPING.md).
Axis 6 produced no connected D4-D11 signal. The CNC controller deliberately
accepts only X, Y, and Z.

## Decoder status

`pendant_decoder/pendant_decoder.ino` is the P3 decoder. It has compiled
successfully, but it has **not** been uploaded during development of the CNC
controller. Uploading it changes the Nano firmware and must be a separate,
explicit action. The previously uploaded raw mapper must not be mistaken for
P3 output.

The decoder only reads the pendant and sends serial packets; it contains no
LinuxCNC, HAL, Mesa, or motion command.

`monitor.py` is also monitor-only. It parses P3 and prints changes without any
motion interface.

## P3 serial protocol

The Nano reports absolute state every 20 ms:

```text
P3,sequence,milliseconds,detent_count,transition_count,quadrature_errors,latest_detent_signal,axis,multiplier,deadman_held,estop_pressed,selector_valid
```

Example:

```text
P3,42,840,-2,-8,0,-1,X,X10,1,0,1
```

`latest_detent_signal` is exactly `-1`, `0`, or `1`. During each 20 ms poll the
Nano retains only the most recent complete detent direction, reports that one
slot, and clears it. Any number of wheel events inside one poll therefore
becomes at most one movement request; it cannot become a movement queue.

When the axis selector is physically OFF and the side button is released, P3
reports `axis=N,multiplier=N,deadman=0,selector_valid=0`. While the side button
is held at OFF, the selected multiplier line becomes observable; for x1 this is
reported as `axis=N,multiplier=X1,deadman=1,selector_valid=0`. This partial
selector report is used only by the locked E-stop recovery sequence and is not
a valid CNC jogging selection.

The Pi establishes a fresh baseline after every Nano boot, selector/deadman
activation, and completed limit recovery. It never converts the first packet
after a reset into machine movement.

Selector and side-button transitions now publish an invalid, non-commanding
state immediately and must remain stable for 20 ms before becoming valid.
Pending and partial wheel motion is discarded at both ends of that interval
and at each E-stop edge, so a detent fragment can never cross into a new axis,
deadman, or E-stop state.

## Files

- `pendant_decoder/pendant_decoder.ino`: P3 Nano firmware.
- `pendant_protocol.py`: strict P3 parser and signed counter-wrap handling.
- `monitor.py`: P3 monitor with no CNC control imports.
- `raw_mapper/raw_mapper.ino` and `raw_monitor.py`: the earlier empirical
  mapping tools.
- `../../reference/legacy_controller/`: the preserved direct-control
  implementation used only as a behavioral reference.
