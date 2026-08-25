# Pendant CNC control

`control.py` converts the measured MYST1474/Nano P3 packets into finite Mesa
7I95 stepgen targets. It has an explicit non-live default: no serial port, HAL
thread, Mesa board, or output is opened unless `--live` is present.

## Verified machine mapping

| Pendant axis | Mesa motor | Positive physical direction | Mapped limit |
|---|---:|---|---:|
| X | 1 | left | IN11 |
| Y | 0 | forward | IN9 |
| Z | 2 | up | IN10 |

All three mapped switches are at the positive end of their axis. Therefore
positive motor counts are "toward the switch" and negative counts are "away
from the switch." Axis 4, axis 5, invalid selector states, and the unconnected
axis 6 cannot command a CNC motor.

## Handwheel scaling

The fixed pulse request per full handwheel detent is:

| Multiplier | Machine step pulses per detent |
|---|---:|
| x1 | 10 |
| x10 | 100 |
| x100 | 1000 |

At the confirmed 4000-pulse/revolution motor setting, the user-requested
independent rate scaling changes x1 from 2500 to 5000 pulses per second, x10
from 15000 to 75000 pulses per second, and x100 from 30000 to 300000 pulses per
second. These requested rates are separate from the fixed
10/100/1000-pulse distance. With the nominal 4 mm screw lead those detents are
0.01, 0.1, and 1 mm respectively.

Movement is latest-wins rather than queued. The Nano's 20 ms poll retains only
the last complete detent direction and emits at most one command signal. On the
Pi, a new signal replaces the outstanding target with
`current_generated_count + selected_increment`; it is never added to the old
target. Thus the maximum outstanding request is 10, 100, or 1000 steps, even if
an input glitch produces an enormous number of encoder events.

The user-confirmed physical handwheel direction is fixed per axis:

| Handwheel | X | Y | Z |
|---|---|---|---|
| clockwise | right / motor negative | forward / motor positive | up / motor positive |
| counterclockwise | left / motor positive | backward / motor negative | down / motor negative |

The user confirmed the side button as the hold-to-jog dead-man control:
releasing it cancels the outstanding jog target and discards released wheel
changes; holding it establishes a new count baseline before permitting
movement.

## E-stop recovery without restarting

Pressing/opening the pendant E-stop immediately cancels all commanded motion
and enters an input-only recovery lock. Releasing the E-stop does not restore
motion by itself. The controller requires this exact sequence:

1. Put the axis selector on X and the multiplier selector on x10.
2. Move the multiplier selector from x10 to x1 while the axis remains X.
3. Move the axis selector from X to OFF while the multiplier remains x1.
4. Rotate the wheel clockwise by at least one completed detent.
5. Rotate the wheel counterclockwise by at least one completed detent.
6. Press and release the side button three times.

All stepgen command-enable signals remain false for the entire sequence. The
x10-to-x1 transition is observed while axis X makes the multiplier electrically
visible. With the axis selector OFF, x1 is also verified whenever the side
button is held. Each click is counted only after a complete debounced press and
release. An out-of-order action, quadrature error, or E-stop re-press restarts
recovery at X+x10. A live or latched limit prevents unlock. After the third
release, the normal controller requires a fresh selector/dead-man baseline
before accepting a wheel detent.

## Limit collision and exact recovery

Each raw input is latched in the 1 ms realtime HAL thread, independent of
Python polling. For a commanded positive move on the matching axis:

1. The realtime gate removes that stepgen's enable as soon as its switch is
   detected.
2. The pending handwheel target is discarded and all three command enables are
   cleared.
3. The stopped generated count is captured.
4. That same motor receives one position-mode target of exactly
   `stopped_count - 250`, at 1500 pulses per second.
5. Its own latched switch permits only that negative/away move. Either of the
   other two switches still blocks it.
6. The latch and pendant baseline reset only after the generated delta is
   exactly `-250` and the raw switch has cleared.

The controller does not guess in ambiguous cases. A limit already active at
startup, a limit from a different axis, multiple limits, a limit during an
away move, a switch still active after 250 counts, or a non-exact generated
count stops the process without starting another recovery move.

This guards the three switches that actually exist; it cannot create an
unwired limit at the opposite end of an axis.

## Other fail-closed gates

- A 100 ms realtime heartbeat watchdog is in series with every stepgen enable.
- A missing/invalid P3 stream cannot start motion.
- A Nano reboot, serial loss, quadrature error, E-stop, or HAL fault cancels
  active commands. E-stop alone keeps the process alive in its locked recovery
  state; the other listed faults remain process-stopping faults.
- LinuxCNC or another `halrun` process must not already be active.
- The serial port must not already be owned by another monitor.

## Invocation

Configuration validation only (no hardware access):

```bash
python3 /home/kit/pendant_cnc/control.py \
  --board-ip 192.168.1.121
```

The board IP shown is already the default, so validation can also be run as
`python3 /home/kit/pendant_cnc/control.py`. Only append `--live` after P3 is
actually uploaded to the Nano. Jogging requests 5000 pulses per second for x1,
75000 for x10, and 300000 for x100. The 250-pulse bounce remains at 1500
pulses per second.

## Verification

- All parser, scaling, mapping, latest-wins, limit, and E-stop recovery
  state-machine tests pass.
- The same production HAL gate topology passes a realtime software simulation:
  toward motion is blocked by its own latch, away recovery is allowed, another
  limit blocks recovery, and heartbeat loss disables the output gate.
- The P3 Nano firmware compiles for the attached Nano target.
- The Nano partial OFF/x1 side-button state was confirmed monitor-only on the
  attached pendant.
- The complete E-stop recovery sequence was confirmed live: the process stayed
  running, remained locked through every prior stage, counted all three full
  clicks, and unlocked only after the third release.
