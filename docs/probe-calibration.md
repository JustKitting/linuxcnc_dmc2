# Probe calibration

This file records the user-accepted probing calibration data stored in
`live/dmc2.ini` under `[PROBE_CALIBRATION]`. It authorizes no machine action.

## Stored values and convention

All values use millimetres. `PROBE_OFFSET_X_MM` and `PROBE_OFFSET_Y_MM` are the
signed side-probe centre position relative to the spindle centre in LinuxCNC
axis coordinates. They are calibration vectors, not direct motion commands.

| Field | Value |
| --- | ---: |
| `PUCK_HEIGHT_MM` | `19.400000` |
| `PROBE_OFFSET_X_MM` | `-51.137072` |
| `PROBE_OFFSET_Y_MM` | `+14.992500` |
| `CALIBRATION_CUTTER_DIAMETER_MM` | `9.230000` |
| `SIDE_PROBE_DIAMETER_MM` | `9.710000` |
| `RADIUS_CORRECTION_MM` | `+0.240000` |

The shared radius correction is:

```text
(side probe diameter - calibration cutter diameter) / 2
= (9.710000 - 9.230000) / 2
= +0.240000 mm
```

## Puck height

`PUCK_HEIGHT_MM = 19.400000` is USER-OBSERVED ACTUAL: the user's measurement
from the machine plate to the puck contact face. The puck sits directly on the
machine plate for the planned Z reference.

## X centre offset

The two 2026-09-04 contact triggers were retained and read back before this
calculation:

- spindle/cutter trigger: machine X `96.069630 mm`;
- side-probe trigger: machine X `147.446702 mm`; and
- both records identify LinuxCNC parameter `#5061`, millimetres, the active
  work frame, physical RIGHT, and LinuxCNC `-X`.

The stored X centre offset is:

```text
spindle trigger X - side-probe trigger X + radius correction
= 96.069630 - 147.446702 + 0.240000
= -51.137072 mm
```

The retained runtime evidence is:

- `var/log/linuxcnc/probe-offset-x-spindle-physical-right-20260904-result.txt`;
- `var/log/linuxcnc/probe-offset-x-spindle-physical-right-20260904.txt`;
- `var/log/linuxcnc/probe-offset-x-side-probe-physical-right-20260904-result.txt`;
  and
- `var/log/linuxcnc/probe-offset-x-side-probe-physical-right-20260904.txt`.

Those runtime paths are ignored by Git. Their exact accepted trigger operands
and calculations are duplicated here so the versioned calibration record does
not depend on ignored log retention.

## Y centre offset

The retained Y contact operands are:

- spindle/cutter trigger: machine Y `94.221496 mm`; and
- side-probe trigger: machine Y `79.468996 mm`.

The stored Y centre offset applies the same radius correction as X:

```text
spindle trigger Y - side-probe trigger Y + radius correction
= 94.221496 - 79.468996 + 0.240000
= +14.992500 mm
```

The retained source analysis is
`docs/2026-09-03-false-puck-movement-claim-postmortem.md`. That record also
keeps a critical distinction: `+14.992500 mm` is the centre-to-centre probe
vector, not the operational move for the current `9.230000 mm` cutter. The
user-observed alignment direction in the 2026-09-04 calibration sequence was
physical BACKWARD / LinuxCNC `-Y`; no positive-Y motion is authorized by this
stored vector.
