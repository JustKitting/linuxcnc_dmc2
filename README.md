# DMC2 LinuxCNC integration

This directory contains the accepted provisional LinuxCNC profile for one
controller architecture:

```text
MYST1474 pendant -> Arduino Nano decoder -> USB serial -> LinuxCNC on Raspberry Pi
LinuxCNC on Raspberry Pi -> Ethernet -> Mesa 7I95T -> machine I/O and axes
```

LinuxCNC is the only Mesa owner. The Nano is an input bridge; it does not run
LinuxCNC, load HostMot2, or command an axis by itself. Obsolete direct-Mesa and
offline reference controllers are not part of this repository.

## Repository layout

- `config/`: reviewed machine constants consumed by compiled code.
- `live/`: the accepted LinuxCNC profile, HAL, UI, and NC programs.
- `rust/crates/`: realtime policy, LinuxCNC interfaces, serial bridge, and task
  diagnostics split into independent crates and responsibility modules.
- `python/dmc2_axis/`: AXIS-required presentation only; never the live launch
  or motion-control boundary.
- `firmware/`: Nano source and Mesa firmware images.
- `tests/linuxcnc-motion/`: isolated real-LinuxCNC motion-consumer fixture.
- `scripts/`: release build, real motion verification, and module installation.
- `patches/`: provenance-locked post-release fixes applied to the pristine
  LinuxCNC 2.9.10 source only in an ephemeral build tree.
- `docs/`: architecture, signal audits, and historical machine notes.
- `archive/local/`: ignored one-off diagnostic programs and the preserved old
  home-directory VCS history.
- `artifacts/`: ignored hardware captures, firmware builds, and backups.
- `var/log/` and `var/tmp/`: ignored persistent diagnostics and disposable
  project scratch files.
- `vendor/`: the clean, commit-locked official LinuxCNC 2.9.10 checkout.

The runtime data flow is documented in `docs/architecture.md`.

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

The Manual Control tab's compact **Homing:** section presents the catalogued
**Home All** control and Known/Unknown position indicators beneath the stock
spindle controls. Its button retains AXIS's existing Home All command binding;
it invokes this same configured LinuxCNC sequence and does not contain a second
homing implementation or substitute different motion values.

IN11, IN9, and IN10 are each shared as that joint's positive limit and home
switch. The normal Cartesian limits stay at 300/173/135 mm; only each joint's
homing range reaches its accepted switch coordinate 0.25 mm farther positive.

## Pendant control

`live/pendant.hal`, the compiled `dmc2-serial-bridge`, the compiled
`dmc2-task-monitor`, and the no-`std` `dmc2_rt.so` realtime component provide
pendant input through LinuxCNC's native servo-thread wheel-jog interface. HALUI
is used only for acknowledged machine-state requests; Python has no
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
Raw reads use fixed 256-byte chunks. The separate framing state retains at most
128 payload bytes plus one possible terminal CR, faults closed as soon as a
frame is provably overlong, and discards through the next LF. Every 8-bit input
value, CR/LF boundary, and 127/128/129-byte boundary is covered by the compiled
serial tests, including a pseudo-terminal pass through the direct Rust/POSIX
termios adapter. The Rust serial boundary preserves Linux errno values for open,
read, configuration-cleanup, and close failures; the bridge reports each
failure instead of collapsing or discarding it.

- The side button must be held to jog.
- x1 issues a 10-pulse target at 5000 target pulses/s (the preceding 2500 rate x2).
- x10 issues a 100-pulse target at 75000 target pulses/s (the preceding 15000 rate x5).
- x100 issues a 1000-pulse target at 300000 target pulses/s (the preceding 30000 rate x10).

These are rates for pacing the finite wheel-count target into LinuxCNC, not a
claim that the physical axis reaches those velocities during a short move.
LinuxCNC 2.9.10's wheel-jog HAL API has no numeric per-command velocity pin.
Its planner still limits every axis to 30 mm/s and 50 mm/s², so acceleration,
distance, and the machine ceiling remain authoritative. The compiled timeout
envelope is derived from those live INI planner limits and the target-issuance
durations, with the existing two-second floor retained.

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
then issued at 1500 target pulses/s. The supervisor requires the Mesa
feedback to finish within the same 20% tolerance as other manual pendant
positioning. If the raw switch clears, it resets only that safety latch. If the
exact automatic backoff completes while the raw switch remains active, no
additional motion is invented: the controller retains attribution, keeps
LinuxCNC's native hard-limit input masked, and accepts only deadman-held pendant
detents on that same axis in the negative/away direction. Other axes and the
toward direction remain blocked. Once the raw input clears, the latch resets and
normal pendant control returns. Homing and non-pendant motion retain the normal
hard-limit input path. Startup uses the same recovery when it finds exactly one
matching raw-and-latched limit. Wrong-axis, multiple, unattributed, incoherent,
or timed-out motion still faults closed.

After a pendant E-stop, recovery requires exactly:

```text
X+x10 -> X+x1 -> OFF -> clockwise -> counterclockwise -> 3 complete side-button clicks
```

The pendant has no separate E-stop state. Pressing its physical E-stop drops
the gate feeding LinuxCNC's canonical `estop_latch`, so the standard AXIS
E-stop state is asserted. After physical release, either AXIS Reset or the
completed pendant gesture re-arms that same latch. The pendant gesture also
requests Machine On; AXIS Reset does not. A 100 ms realtime heartbeat watchdog
faults the same E-stop chain if the userspace controller freezes.

## AXIS status panel

The toolbar's pendant icon is enabled only after the controller reports that
the startup gate, LinuxCNC state, Nano link, and limits are ready. Selecting it
first requests Pendant Mode; the PyVCP panel appears only after the controller
returns its post-arm `control-ready` acknowledgement. The request is removed
and the panel is hidden only when the controller itself becomes unavailable.
During an expected limit collision, automatic backoff, or attributed manual
release, the control session remains available and the panel stays open while
`control-ready` temporarily drops; it returns automatically after the raw
switch clears and the bounce resets the latch.
Bounce completion requires the latch-reset output to remain high for 10 ms,
then remain low for another 10 ms before the controller returns to idle. Fault
and E-stop paths force every latch-reset output low, preventing a completed or
interrupted bounce from masking the next physical limit event.
The supervisor also reads each HostMot2 stepgen's fractional position feedback.
It places the negative-bounce endpoint in the middle of the target integer
count bucket and evaluates completion against the 20% manual-motion tolerance
before deciding whether to reset the latch or await an operator-commanded
release increment.
The exact LinuxCNC `Jog aborted by jog-stop-immediate` and
`Jog aborted by jog-stop` operator notifications produced by the controller's
intentional realtime limit stop and controlled pendant cancellation are
suppressed in AXIS, while every other error remains visible. A homing-state
cancellation publishes the controlled stop only once while LinuxCNC
decelerates and settles. The panel shows:

- top-level control-ready, fault, recovery, jog, and limit-backoff state;
- LinuxCNC X/Y/Z machine-coordinate feedback;
- Mesa-generated pulse counts, explicitly labelled as pulses;
- live and independently realtime-latched IN11/IN9/IN10 limit status;
- live and realtime-latched IN0 puck and IN1 DMC2-probe contact status;
- OUT5 probe-power state;
- Nano connection, link, wheel-decoder, E-stop, deadman, and selector status;
- only physical selector positions, with X/X1, Y/X10, and Z/X100 vertically
  aligned.

Known/Unknown homing state is deliberately outside the pendant panel. It stays
visible with **Home All** in the Manual Control tab's compact **Homing:** row.

`CLEAR SEEN` resets only the five display-history latches. It
does not clear a motion-safety latch or bypass a limit.

The always-visible **CLEAR FAULT** toolbar button sends LinuxCNC's canonical
E-stop Reset request; it does not use a separate reset state. After the
physical pendant E-stop is released, this clears a retained controller fault
and re-arms the same canonical latch. It does not home, move an axis, start the
spindle, or turn Machine On.

## LinuxCNC contact-connectivity tests — no motion

USER-OBSERVED ACTUAL calibration facts currently recorded:

- the puck measures **19.40 mm** from the plate to its contact face;
- the installed cutter marking is **10mm-60L**; and
- the puck sits directly on the machine plate for the planned Z reference.

The operator-facing connectivity test is the **CONTACT CONNECTIVITY - NO
MOTION** row in the LinuxCNC panel. It works while the machine is unhomed and
does not request homing, axis motion, or spindle motion. The spindle must
already be stopped. IN0 is the puck contact; IN1 is the DMC2 side-probe
contact. IN0 also drives `motion.probe-input` and `motion.digital-in-00`.

Each contact is tested in a separate run. First separate the grounded clip or
clipped tool from both contact surfaces and press **CLEAR SEEN**. Press **START
TEST**, then touch the grounded clip or clipped tool to either the puck or the
DMC2 side probe. The yellow **ACTIVE** and OUT5 indicators show the finite
test-power window. A real IN0 contact sets **Puck / IN0 SEEN**; a real IN1
contact sets **DMC2 probe / IN1 SEEN**. Either contact removes the test-power
request immediately. **STOP TEST** also removes the request. With no contact
or stop request, the realtime one-shot removes it after exactly 300 seconds;
another START press cannot extend an active window. Clear the display latch
and start a new run before testing the other contact.

`motion.digital-in-01` reports that internal OUT5 gate state; it is not the
physical Mesa IN1 terminal. The panel test's START and STOP pins and both
contact inputs connect only to the realtime one-shot/output gate and display
latches, never to a joint, axis, homing pin, or motion command.

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
user-verified top-down physical mapping is H100 explicit Forward `0002H` =
CCW and H100 Reverse `0004H` = CW, so LinuxCNC follows the milling standard by
mapping `M3` to `0004H` (CW) and `M4` to `0002H` (CCW). The generic H100
Operation value `0001H` is not used as a direction command because it does not
clear a previously selected direction. Direction-qualified running and
at-speed feedback require H100 `0210H` to match that requested mapping. A
mismatch prevents cutting startup; a direction mismatch after confirmed
running latches a typed spindle fault and commands STOP.

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
the requested valid RPM with `M3`, requires H100 direction feedback to confirm
the physically verified clockwise state, waits for at-speed feedback, commands
`M5`, and does not finish until the H100 reports stopped.
It contains no X/Y/Z motion. Beside AXIS's stock spindle controls, `Actual RPM`
displays the same H100 output-frequency feedback used by
`spindle.0.speed-in`; it remains visible while the pendant panel is closed.
At the top of the hideable pendant panel, `VFD disconnected` asserts when
hm2_modbus has disabled a required command after repeated communication
failures. The same hardware section reports VFD readiness, Mesa
packet/watchdog fault state, and the X/Y/Z HostMot2 step-output enables. It
explicitly reports X/Y/Z physical power and motion feedback as `NOT WIRED`;
those enable signals are not mislabeled as physical motor response.

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

## Isolated real-LinuxCNC validation

The complete verifier refuses to run while another LinuxCNC realtime host is
active. It opens neither USB serial nor Mesa hardware:

```bash
cd <project-root>
scripts/verify.sh
```

It builds the complete Rust release with warnings denied, verifies that all
deployed DMC2 binaries byte-match that release, and starts LinuxCNC 2.9.10 with
real `motmod` plus a software step generator. A PTY supplies raw P3 packets to
the deployed serial bridge. The fixture sources the same pendant input and
motion HAL contracts as the live profile and observes LinuxCNC-owned joint
commands and downstream step counts. Its 20 data-driven moves cover X/Y/Z,
x1/x10/x100, both directions, and consecutive X/x1 commands. Three additional
paths exercise real LinuxCNC jog-stop handling, the production controller's
X/Y/Z limit bounce, toleranced completion, raw-limit-held recovery, restricted
away-direction x1 jog, native hard-limit masking, and safety-latch reset. The
same run starts real LinuxCNC 2.9.10 homemod while a real jog is active and
requires exactly one controlled-stop event across the complete level-active
homing interval. A separate real homemod path holds a shared home/positive-limit
input active long enough to cross multiple task-monitor publications and
requires the production diagnostic journal to remain free of hard-limit
warnings during that joint's homing state. It then validates every real
task-monitor error-journal event with the production AXIS suppression policy,
requires exactly one controlled stop and three immediate stops while leaving
unrelated LinuxCNC messages visible, and forbids LinuxCNC's native
joint-limit error. It loads no
HostMot2 driver, so it cannot address the physical Mesa card or prove physical
switch or motor behavior.
`native/bin/dmc2-linuxcnc` is the standard compiled launcher. With no argument
it validates launch inputs and deployment identity only. The literal `--live`
flag is required before it
can replace itself with LinuxCNC, and it checks for a conflicting LinuxCNC,
HAL, or legacy direct-Mesa owner first. No Python launcher or Python subprocess
exists in either live-launch path.

For live mode, the compiled launcher first proves no LinuxCNC/HAL owner is
active, invokes the realtime-module installer automatically only when required,
and re-verifies all installed module files before starting LinuxCNC. No
separate module-install command is part of the operator workflow. The installer
refuses to run while `rtapi_app` is active, stages and byte-checks
`dmc2_rt.so`, `h100_spindle.so`, and the reviewed LinuxCNC 2.9.10
`hm2_eth.so` hardening overlay beside their installed targets, takes
byte-verified rollback copies, and uses atomic same-filesystem renames. The
custom replacements and driver replacement form one transaction: any commit,
verification, host-state, exit, or signal failure restores every changed
target, while an incomplete rollback preserves its recovery files. The
launcher then byte-compares all three installed modules against the exact release
artifacts. Abnormal process-probe results fail
installation instead of being treated as proof that LinuxCNC is stopped.

The driver overlay is pinned in `config/linuxcnc-driver-overlays.tsv`. It
backports the `hm2_eth` portions of upstream commits `10dc650ad`,
`cd8eb00ad`, and `05de5742`: bounds checks for queued Ethernet buffers,
unconditional write-queue reset after a failed `send()`, and initialization-
aware read/write confirmation. Upstream states that the old accumulating
buffer path generated a segfault; the third change prevents a false soft error
before the first queued write. The build verifies the pristine 2.9.10 base,
patch checksum, installed LinuxCNC version, and exported-symbol contract, then
produces a reproducible staged module without editing the vendor checkout.

For a live GUI/controller that is owned by the user service manager instead of
the initiating terminal, use the explicit persistent form:

```bash
native/bin/dmc2-linuxcnc --live --persistent
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

The compiled launcher runs LinuxCNC beneath `dmc2-session-supervisor`, a Linux
child subreaper, while the live INI/HAL configuration places
`dmc2-process-supervisor` directly around `milltask`, I/O, HALUI, AXIS, and the
two DMC2 userspace adapters. Their shared, locked journal at
`var/log/linuxcnc/process-lifecycle.tsv` retains catalogued ownership, exact
invocations, zombie-safe owner identity, running and terminal-before-reap
`/proc`/cgroup snapshots, independently checked `waitid` and `wait4` status,
signal/core policy, resource usage, matching LinuxCNC task backtraces, and
identity-checked copies of file-based kernel cores. The trackers do not
restart, stop, signal, or otherwise control the machine; see
`docs/process-lifecycle-tracking.md` for the exact evidence boundary.

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
