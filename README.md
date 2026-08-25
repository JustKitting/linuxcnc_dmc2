# DMC2 LinuxCNC integration

This directory contains the accepted provisional LinuxCNC profile for one
controller architecture:

```text
MYST1474 pendant -> Arduino Nano decoder -> USB serial -> LinuxCNC on Raspberry Pi
LinuxCNC on Raspberry Pi -> Ethernet -> Mesa 7I95T -> machine I/O and axes
```

LinuxCNC is the only Mesa owner. The Nano is an input bridge; it does not run
LinuxCNC, load HostMot2, or command an axis by itself. The older
`/home/kit/pendant_cnc/control.py` direct-Mesa program must never run at the
same time as this LinuxCNC profile.

## Accepted provisional machine profile

The profile in `live/dmc2.ini` uses exactly the values accepted on 2026-08-22:

| Axis | LinuxCNC joint | Mesa stepgen | Positive direction / switch | Normal software zone | Switch coordinate | Final home |
|---|---:|---:|---|---:|---:|---:|
| X | 0 | 1 | left / IN11 | 0..300 mm | 300.25 mm | 300 mm |
| Y | 1 | 0 | forward / IN9 | 0..173 mm | 173.25 mm | 173 mm |
| Z | 2 | 2 | up / IN10 | 0..135 mm | 135.25 mm | 135 mm |

All axes use the accepted provisional scale of 1000 pulses/mm, maximum
velocity of 30 mm/s, and maximum acceleration of 50 mm/s². The 1000-pulse/mm
scale passed the user's rough caliper sanity check, but it remains provisional
until the later fine distance calibration.

The standard AXIS **Home All** command homes X, then Y, then Z. Each joint uses
the accepted sequence as one LinuxCNC homing operation:

1. Search in the positive direction at 5 mm/s (5000 pulses/s).
2. Clear the switch and approach it again in the positive direction at
   0.25 mm/s (250 pulses/s).
3. Finish 0.25 mm in the negative direction at 0.25 mm/s: exactly 250 pulses
   away from the switch coordinate.

The status panel's **HOME ALL** button is a momentary route to
`halui.home-all`; it invokes this same configured LinuxCNC sequence. It does
not contain a second homing implementation or substitute different motion
values.

IN11, IN9, and IN10 are each shared as that joint's positive limit and home
switch. The normal Cartesian limits stay at 300/173/135 mm; only each joint's
homing range reaches its accepted switch coordinate 0.25 mm farther positive.

## Pendant control

`live/pendant.hal`, the compiled `dmc2-serial-bridge`, the compiled
`dmc2-task-monitor`, and the no-`std` `dmc2_rt.so` realtime component provide
pendant input through LinuxCNC's native HALUI/motion interfaces. Python has no
authority in the live pendant motion path and no component directly writes a
Mesa motion command.

AXIS starts with **Pendant Mode off** and the PyVCP status panel hidden. The
24-pixel pendant/handwheel button immediately to the right of **Clear live
plot** is the single mode control: dark and raised means off; green and sunken
means on. Turning the mode on shows the panel, publishes the AXIS-owned HAL
enable request, and requires a fresh selector/deadman baseline before accepting
a wheel jog. Turning it off first removes that HAL request, stops any ordinary
active pendant jog, discards the one pending wheel request, and hides the
panel. **Ctrl-E** and the View-menu PyVCP entry invoke the same mode transition,
not a separate visibility-only state. Pendant E-stop processing, realtime
limit detection, and an already-required limit bounce remain active with the
panel hidden and Pendant Mode off.

The USB bridge brackets its individually written HAL fields with an odd/even
snapshot generation. The servo-thread supervisor consumes a packet only when the
generation is unchanged and even before and after the read; a selector,
detent, E-stop, and sequence from different Nano reports cannot be combined.
Serial reads are capped at 128 bytes, above the maximum valid P3 packet, so a
corrupt device cannot grow an unbounded line buffer.

- The side button must be held to jog.
- x1 requests 10 pulses at 5000 pulses/s (the preceding 2500 rate x2).
- x10 requests 100 pulses at 75000 pulses/s (the preceding 15000 rate x5).
- x100 requests 1000 pulses at 300000 pulses/s (the preceding 30000 rate x10).

The requested jog velocity and the accepted machine velocity ceiling are
separate settings. The LinuxCNC profile still limits every axis and the
trajectory planner to 30 mm/s; this rate change does not raise that ceiling or
change acceleration, jog distance, or limit-bounce behavior.

- LinuxCNC retains the 50 mm/s² axis/joint acceleration limit. HostMot2's
  redundant stepgen acceleration limiter is disabled (`maxaccel=0`) so the
  hardware step generator follows that trajectory directly. The unchanged
  0.050/0.010 mm following-error thresholds remain active.
- The active 7I95T firmware's DPLL latches stepgen position 100 µs before
  each nominal Ethernet read. If `hm2_eth` reports a failed current packet,
  only that stale-feedback cycle uses the matching motion command as feedback,
  as prescribed by the installed `hm2_eth` manual; packet-error totals are
  logged by the pendant controller.
- Clockwise means X right/negative, Y forward/positive, or Z up/positive.
- A same-axis, same-direction detent replaces the outstanding target with one
  increment from the current generated pulse count without stopping. LinuxCNC
  receives only the target extension already earned by physical progress, so
  repeated samples cannot grow an event queue. Axis changes and direction
  reversals stop first and retain at most one replaceable pending request.
- Pendant jogging is accepted when LinuxCNC is on, idle, and in manual mode.
  Before all three axes are homed it uses LinuxCNC joint-jog mode; after all
  three axes are homed it uses Cartesian/teleop mode. Unhomed coordinates stay
  explicitly marked unknown in the status panel.

Startup is automatic and fail-closed. The controller first waits for proof
that the servo thread and `hm2.write` have run for 100 ms. It then clears a
stale HostMot2 watchdog bite through the driver's bidirectional status pin,
requires that status to remain healthy for 100 ms, resets all three stale
limit-event latches for 10 ms, and allows another 10 ms for live inputs to
settle before interpreting any limit. While the external E-stop gate remains
closed, the controller waits for AXIS's final post-GUI HAL operation, a valid
LinuxCNC status snapshot, and a healthy Nano snapshot. It then holds the
realtime userspace-watchdog enable low for 10 ms, arms it, and requires its
`ok-out` feedback to remain continuously true for 250 ms. The heartbeat
changes level every 20 ms, so each level spans many 1 kHz servo samples instead
of toggling once per userspace loop. Startup readiness has no maximum elapsed
time: Mesa readiness, AXIS/Nano/LinuxCNC prerequisites, watchdog recovery,
LinuxCNC E-stop reset, and machine-on are all awaited by state while the
external gate is closed whenever watchdog health is absent. If the controller
watchdog drops before reset and machine-on have explicitly completed, startup
closes the gate, cleanly re-arms the watchdog, and retries the automatic reset.
The controller publishes reset and machine-on requests as HAL signals to
HALUI, so those LinuxCNC state commands cannot block the userspace loop that
supplies the 100 ms heartbeat. Each request remains asserted until LinuxCNC
status acknowledges the requested state, and every retry or fail-closed path
clears it before a new edge is allowed.
Only after that completion event is the watchdog committed to runtime, where a
subsequent timeout faults and is never automatically cleared. The Mesa and
controller watchdog timeouts remain 100 ms; neither safety timeout is
lengthened. Minimum electrical settling intervals and bounded commanded-motion
checks remain separate from startup readiness. A completed startup requires a
fresh pendant baseline before jogging.

For an attributable pendant collision with the selected axis's positive limit,
the realtime input latch records the event and the old controller's directional
truth table drives LinuxCNC's immediate-jog-stop input: the matching own limit
blocks only the toward direction, the negative bounce direction remains
permitted, and either other limit blocks the active motor. During that already
attributed pendant operation only, the directional gate replaces LinuxCNC's
default machine-off hard-limit response. One negative 250-pulse recovery jog is
then requested at 1500 pulses/s. The supervisor requires the Mesa
generated-count delta to be exactly -250 and the raw switch to clear before it
resets only that safety latch. Homing and non-pendant motion retain the normal
hard-limit input path. If startup finds exactly one matching raw-and-latched
limit, the supervisor attributes that switch before enabling LinuxCNC and runs
the same exact negative 250-pulse bounce at 1500 pulses/s; this prevents an
already-active switch from locking out its own recovery direction. A
wrong-axis, multiple, unattributed, non-exact,
uncleared, or timed-out event faults closed instead of inventing a recovery
move.

After a pendant E-stop, recovery requires exactly:

```text
X+x10 -> X+x1 -> OFF -> clockwise -> counterclockwise -> 3 complete side-button clicks
```

The gesture itself is input-only. Only after it completes does the supervisor
permit the external E-stop chain and request LinuxCNC E-stop reset/machine-on.
A 100 ms realtime heartbeat watchdog faults the E-stop chain if the userspace
controller freezes.

## AXIS status panel

The toolbar's pendant icon is enabled only after the controller reports that
the startup gate, LinuxCNC state, Nano link, and limits are ready. Selecting it
first requests Pendant Mode; the PyVCP panel appears only after the controller
returns its post-arm `control-ready` acknowledgement. The request is removed
and the panel is hidden only when the controller itself becomes unavailable.
During an expected limit collision and exact backoff, the control session
remains available and the panel stays open while `control-ready` temporarily
drops; it returns automatically after the bounce clears the latch.
Bounce completion requires the latch-reset output to remain high for 10 ms,
then remain low for another 10 ms before the controller returns to idle. Fault
and E-stop paths force every latch-reset output low, preventing a completed or
interrupted bounce from masking the next physical limit event.
The supervisor also reads each HostMot2 stepgen's fractional position feedback.
It places the negative-bounce endpoint in the middle of the target integer
count bucket, so HostMot2 emits exactly 250 negative pulses even when the
accumulator begins on an integer boundary; the final integer count delta is
still checked before the limit latch can be reset.
The exact LinuxCNC `Jog aborted by jog-stop-immediate` operator notification
produced by that intentional realtime stop is suppressed in AXIS, while every
other error remains visible. The panel shows:

- top-level control-ready, fault, recovery, jog, and limit-backoff state;
- LinuxCNC X/Y/Z machine-coordinate feedback;
- Mesa-generated pulse counts, explicitly labelled as pulses;
- live and independently realtime-latched IN11/IN9/IN10 limit status;
- live and realtime-latched IN0 puck and IN1 DMC2-probe contact status;
- OUT5 probe-power state;
- Nano connection, link, wheel-decoder, E-stop, deadman, and selector status;
- only physical selector positions, with X/X1, Y/X10, and Z/X100 vertically
  aligned; and
- a **HOME ALL** button routed to the accepted X-then-Y-then-Z sequence.

`CLEAR SEEN` resets only the five display-history latches. It
does not clear a motion-safety latch or bypass a limit.

## LinuxCNC puck-contact test — no motion

USER-OBSERVED ACTUAL calibration facts currently recorded:

- the puck measures **19.40 mm** from the plate to its contact face;
- the installed cutter marking is **10mm-60L**; and
- the puck sits directly on the machine plate for the planned Z reference.

The operator-facing connectivity test is the **PUCK CONNECTIVITY TEST - NO
MOTION** row in the LinuxCNC panel. It works while the machine is unhomed and
does not request homing, axis motion, or spindle motion. The spindle must
already be stopped. IN0 drives both `motion.probe-input` and
`motion.digital-in-00`.

To run it, first separate the cutter and puck and press **CLEAR SEEN**. Press
**START TEST**, then manually touch the installed cutter to the puck. The
yellow **ACTIVE** and OUT5 indicators show the finite test-power window. A
real IN0 contact sets the orange **Puck / IN0 SEEN** indicator and removes the
test-power request immediately. **STOP TEST** also removes the request. With
no contact or stop request, the realtime one-shot removes it after exactly 300
seconds; another START press cannot extend an active window.

`motion.digital-in-01` reports that internal OUT5 gate state; it is not the
physical Mesa IN1 terminal. The panel test's START and STOP pins connect only
to the realtime one-shot/output gate, never to a joint, axis, homing pin, or
motion command.

`live/nc_files/puck-contact-no-motion-test.ngc` remains as a deeper
interpreter-path verification using LinuxCNC `M64`, `M65`, and `M66`. Because
the live profile intentionally keeps `[TRAJ] NO_FORCE_HOMING = 0`, LinuxCNC
will not start that Auto program while unhomed. It is not the basic unhomed
connectivity-test entry point.

The separate `dmc2_abort.ngc` handler issues `M65 P0` after any LinuxCNC
program abort. This prevents an interrupted test request from becoming active
again when a later program starts. A stopped realtime writer is still covered
by the Mesa watchdog.

This test does not use the 19.40 mm measurement, alter coordinates, populate
the tool table, or perform `G38` motion. It validates only the real electrical
and LinuxCNC path required before a later explicitly approved probing routine.

## First moving tool-height contact test

`live/nc_files/tool-height-first-test.ngc` is the separately approved first
moving validation.  It assumes the machine is already homed and X/Y are
stationary at the recorded puck station, machine X **288.125 mm** and machine
Y **152.955 mm**.  It does not command X or Y.

The program stops at an operator gate before enabling OUT5 or moving Z.  After
the operator resumes it rechecks X/Y and the open puck input, enables OUT5,
and performs exactly one `G38.2` Z-down move at **6 mm/min (0.1 mm/s)** toward
machine Z zero.  A real IN0 contact stops the probe move.  The program records
the probe coordinate, removes OUT5 power, then uses LinuxCNC's normal
acceleration-limited `G53 G0` trajectory to return to machine Z home at
**135.0 mm**.  Puck height is the user-measured **19.40 mm**.

This first test reports the contact and derived coordinates but deliberately
does not change G54, G92, the tool table, or tool-length compensation.  A
program abort invokes `dmc2_abort.ngc`, which clears the OUT5 request. This
complete first sequence was observed and accepted before adding a faster test.

## Homing-style double-touch tool-height test

`live/nc_files/tool-height-homing-style-test.ngc` preserves the same recorded
puck station and requires machine Z home at **135.0 mm** before it can start.
Its first popup is a connection test with **no axis motion**: after the operator
starts that stage, OUT5 turns on and the program waits up to 300 seconds for a
real IN0 transition while the puck is manually touched to the installed tool
tip. OUT5 turns off immediately after the result. Only a successful contact
opens the second popup, while OUT5 remains off, for placing the puck beneath the
tool. Starting that second stage performs the unchanged user-approved sequence:
first probe at **5 mm/s**, back off upward exactly **1.00 mm at 0.25 mm/s**,
re-touch downward at **0.25 mm/s**, remove OUT5 power, and use the normal
acceleration-limited rapid return to Z home. The second contact is the reported
measurement; the first-to-second difference is also reported. It never commands
X, Y, or the spindle.

## Spindle integration

The live profile maps LinuxCNC's standard `S`, `M3`, `M4`, and `M5` state to
the H100 through Mesa PktUART channel 1 and `hm2_modbus`. The realtime
sequencer writes and verifies the frequency while holding STOP, releases the
direction command only after exact `0201H` readback, reports actual RPM and
`at-speed` from the VFD, and sends STOP before clearing frequency. The
user-verified top-down physical mapping is H100 `0001H` = CCW and H100
`0004H` = CW, so LinuxCNC follows the milling standard by mapping `M3` to
`0004H` (CW) and `M4` to `0001H` (CCW).

The stock DMC2 Mini spindle is published as 2.2 kW and 24,000 RPM. A matching
DMC2 Mini/H100 installation records F004=400 Hz, F005=400 Hz, F011=100 Hz,
two poles, and 24,000 RPM. On 2026-08-24 the stopped one-time configuration
changed this machine's live F004/F005 from raw 500/500 (50.0/50.0 Hz) to raw
4000/4000 (400.0/400.0 Hz). After restart LinuxCNC read back observed and
expected F004/F005 as 4000, `h100-spindle.ready=true`, block code 0, STOP=8,
commanded/output frequency 0, and VFD fault 0. The INI maps 24,000 RPM to
400.0 Hz and refuses commands below the 6,000 RPM software minimum.

The reusable `live/nc_files/dmc2_spindle_test.ngc` operation takes the test
speed as its argument instead of embedding one speed. The direct LinuxCNC call
is `o<dmc2_spindle_test> call [RPM]`. It starts from stopped feedback, commands
the requested valid RPM with `M3`, waits for the H100 running and at-speed
feedback, commands `M5`, and does not finish until the H100 reports stopped.
It contains no X/Y/Z motion.

## Deliberately disabled or deferred

- IN0 is connected to `motion.probe-input`; implemented probing motion is
  limited to the two explicitly approved Z-only tests above.
- IN1 remains status-only and is not connected to `motion.probe-input`.
- OUT5 can be requested by LinuxCNC digital output 0 while a program is
  running, or by the unhomed panel test's non-retriggerable 300-second maximum
  window. The panel source clears immediately on IN0 contact or **STOP TEST**;
  the program source clears on success, timeout, and program abort.
- The H100 parameter correction and direct LinuxCNC clockwise spindle RUN at
  requested speed have been physically verified; each new tool/work setup
  still requires the operator's normal pre-cut check.
- No motor-alarm input is assigned because its input/polarity is not yet
  mapped.
- No Mesa SSR is assigned to the shared physical drive enable; only the three
  internal HostMot2 stepgen enable pins follow LinuxCNC joint enable.

These are non-blocking for the accepted axis/pendant/GUI profile and remain
explicitly disabled—not silently guessed.

## Hardware-free validation

The following commands open neither USB serial nor Mesa hardware:

```bash
cd /home/kit
python3 -m unittest discover -s linuxcnc_dmc2 -p 'test_*.py' -v
linuxcnc_dmc2/native/bin/dmc2-task-monitor --validate
python3 linuxcnc_dmc2/validate_offline.py
python3 linuxcnc_dmc2/check_live_readiness.py
python3 linuxcnc_dmc2/launch_live.py
```

The last command is validation-only unless the literal `--live` flag is
present. It checks for a conflicting LinuxCNC, HAL, or legacy direct-Mesa
owner before its explicit live path. The construction and validation work did
not run that path.

For a live GUI/controller that is owned by the user service manager instead of
the initiating terminal, use the explicit persistent form:

```bash
python3 /home/kit/linuxcnc_dmc2/launch_live.py --live --persistent
```

This creates the transient `dmc2-linuxcnc.service` unit with no automatic
restart. Its log is available with `journalctl --user-unit dmc2-linuxcnc`, and
`systemctl --user stop dmc2-linuxcnc.service` stops the complete LinuxCNC
process group. The launcher explicitly passes `LINUXCNC_FORCE_REALTIME=1` to
the transient service. This is required on this Pi's PREEMPT_RT kernel because
LinuxCNC 2.9.10's legacy automatic probe also requires the absent
`/sys/kernel/realtime` file. `/usr/bin/rtapi_app` must remain root-owned and
setuid (`root:root`, mode `4755`) or LinuxCNC will deliberately fall back to
POSIX non-realtime scheduling.

The optional hardware-free HAL integration check is:

```bash
cd /home/kit/linuxcnc_dmc2/sim
halrun offline_hal_smoke.hal
```

`sim/monitor.ini` uses replayed pendant packets, fake positions, and no
hardware driver. Its numerical motion values are simulation-only.

## Sources

- [Mesa 7I95T manual](https://www.mesanet.com/pdf/parallel/7i95tman.pdf):
  terminal mapping and isolated I/O electrical behavior.
- [LinuxCNC homing configuration](https://linuxcnc.org/docs/stable/html/config/ini-homing.html):
  search/latch/final-home and shared home/limit behavior.
- [LinuxCNC INI reference](https://linuxcnc.org/docs/stable/html/config/ini-config.html):
  joint/axis limits, scale, velocity, and acceleration.
- [LinuxCNC Python interface](https://linuxcnc.org/docs/stable/html/config/python-interface.html):
  finite jog, stop, mode, state, and status APIs.
- [LinuxCNC HostMot2 watchdog](https://linuxcnc.org/docs/html/drivers/hostmot2.html):
  watchdog bite behavior, pulled-high I/O state, reset, and periodic
  `hm2.write` requirements.
- [LinuxCNC HostMot2 HAL reference](https://linuxcnc.org/docs/stable/html/man/man9/hostmot2.9.html):
  DPLL stepgen latching and hardware position-control behavior.
- [LinuxCNC hm2_eth HAL reference](https://linuxcnc.org/docs/stable/html/man/man9/hm2_eth.9.html):
  transient packet-error behavior and stale-feedback substitution.
