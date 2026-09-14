# BTER tool setter

## Equipment and retained wiring

The user installed the BTER MY20-20-2 normally closed tool setter, Amazon
[B09V5RNGS2](https://www.amazon.com/dp/B09V5RNGS2).

SOURCE-VERIFIED: the MY20-20-2 schematic on PDF page 2 of the
[model manual](https://storage.ua.prom.st/2359309_datchik_visoti_instrument__bnik_koristuvacha_chi.pdf)
shows brown/orange as the contact pair and green/blue as the overtravel pair.
Terminal naming is from the [Mesa 7I95T manual](https://www.mesanet.com/pdf/parallel/7i95tman.pdf).

USER-OBSERVED ACTUAL, accepted wiring:

| Wire / connection | Terminal |
| --- | --- |
| Brown, contact | TB6 pin 1, IN0 |
| Orange, contact return | TB2 pin 19, GND |
| Green, overtravel | TB6 pin 4, IN2 |
| Blue, overtravel return | TB2 pin 19, GND |
| Fixed +5 V from TB2 pin 22 | TB6 pin 6, INCOM2,3 |
| Existing fixed +5 V common for IN0,1 | TB6 pin 3 |
| Existing XYZ probe signal | TB6 pin 2, IN1 |

The user subsequently reported correcting wiring and seeing a board signal;
the exact correction was not specified. The existing XYZ probe retains its
fixed +5 V supply. OUT5 remains the legacy program output, not the setter supply.

## Input evidence and configuration

Read-only recording: `/tmp/dmc2-tool-setter-record.zhLvpk/samples.tsv`.
It retained 803 samples over 59.171047 seconds. IN0 and IN2 were initially TRUE;
both changed FALSE then TRUE at elapsed 25.794363/26.013678 seconds and again at
28.517932/28.749522 seconds. IN1 stayed FALSE. No sampled Mesa packet-error,
packet-error-exceeded or watchdog flag was set. Both setter transitions appeared
in the same samples; this does not establish their separate mechanical depths.

`hal/tool_setter.hal` feeds both raw NC circuits to the Rust tool-setter policy.
Closed/released raw TRUE becomes contact FALSE; open raw FALSE becomes contact
TRUE. The normalized IN0 contact retains the `dmc2-puck-live` internal alias,
feeds digital input 0 and the existing probe selector. P1 still selects the XYZ
probe; manual Probe Mode selects XYZ only in Manual/idle. Normalized overtravel
is separately available on digital input 6, its live lamp and its SEEN lamp.

An overtravel observation is retained. It asserts `motion.feed-inhibit` and
requests `halui.program.stop` while a program/coordinated mode is active.
LinuxCNC [motion](https://linuxcnc.org/docs/2.9/html/man/man9/motion.9.html) and
[HALUI](https://linuxcnc.org/docs/2.9/html/man/man1/halui.1.html) define these
interfaces. Feed inhibit affects G-code, not manual jogs; it is not a substitute
for hard limits or E-stop. No automatic withdrawal or resume is issued.

A discarded Mesa read preserves the last valid contact sample. Existing Mesa
transport fault handling remains responsible for communication failure. Before
the first valid sample, program feed is inhibited. A normal contact stops a
selected LinuxCNC probe/jog through its existing probe handling; manual contact
bookkeeping covers both selected sensors without adding a pendant fault.

## Visible recovery

The TOOL SETTER RECOVERY panel shows the current state and action. Release the
open circuit; use Abort and Pendant Mode for manual withdrawal. With both
circuits closed, fresh task status, Manual/idle, no homing and axes stationary,
press CLEAR SETTER. The existing Clear Fault request can acknowledge the same
condition. A press made before these conditions is not queued for later.
Neither action resumes a program. CLEAR SEEN clears display history only.

The setter policy does not own E-stop, machine enable, pendant selection or jog
permission. Existing Abort, Clear Fault and Pendant Mode controls remain the
operator recovery path. State-model tests do not establish physical stopping or
operator-observed recovery; those require live observation.

## Earlier top-height reference — superseded on 2026-09-13

The initial XYZ-probe top-height reference is preserved in
[the historical reference](../config/metrology/references/tool-setter-top-height-2026-09-13.json).
Its **63.995466247558595 mm** estimate used the measured height difference and
nominal zero pretravel. The effective IN0 contact calibration below supersedes
that estimate for current tool measurements.

USER-OBSERVED ACTUAL: both recordings used the same XYZ probe with unchanged
clamping depth and homed coordinate reference. All contacts were downward
(physical DOWN / LinuxCNC -Z). These are Mesa IN1 rising-edge servo samples of
LinuxCNC machine XYZ feedback, not post-stop positions or hardware-latched
coordinates. The unchanged probe/tool offset cancels in the difference; there
is no additional ball-radius correction.

| Surface | Machine X (mm) | Machine Y (mm) | Median trigger Z (mm) | Touches | Z spread (mm) |
| --- | --- | --- | --- | --- | --- |
| Setter top | 274.1179066619873 | 130.02257946777343 | 99.144289260864265 | 4 | 0.00853018188476 |
| Plate sample | 288.7293769989014 | 147.64695007324218 | 35.14882301330567 | 5 | 0.003966583251952 |

The calculation retains every touch:

```text
median(setter top) - median(plate) - nominal setter pretravel
= 99.144289260864265 - 35.14882301330567 - 0
= 63.995466247558595 mm
```

SOURCE-VERIFIED: the HMWTECH MY20-20-2 Product parameters table on
[PDF page 22](https://doc.diytrade.com/docdvr/1454255/51348984/1684722298.pdf#page=22)
lists `Pretravel: 0`. This is the user-accepted nominal correction; the recordings
measure XYZ-probe IN1 contact, not the setter's own IN0 switching height. No
stroke, force or other motion setting is taken from that catalog entry.

The setter XY is a measured top contact point, not a measured center. The plate
sample is at the separate XY shown above; this record does not establish a flat
plate plane or a new absolute physical surface Z. X direction labels remain
physical RIGHT / LinuxCNC -X and physical LEFT / LinuxCNC +X. The coordinates
are retained measurements, not motion instructions.

### Retained source evidence

[Exact touch rows and operands](../config/metrology/tool-setter-measurements-2026-09-13.json)
preserve the analysis readback. Both complete recordings, including movement
samples, are archived losslessly in the repository:

| Recording | Uncompressed SHA-256 |
| --- | --- |
| [Setter top](../config/metrology/recordings/probe-1789317770574733990-1027637-1.csv.gz) | `2ec128344d6ced6403fc1d5e4e3cdfbd6be30c354e065329fc40839dab91580f` |
| [Plate](../config/metrology/recordings/probe-1789318065305904576-1027637-2.csv.gz) | `07dcb8c62fe5b3ff11615bf0bb46804282620e3d850c2b9981c7307654ce65e2` |

All rows report valid homed positions, with no cycle gaps or recorded
transport/controller fault flags. The contemporaneous LinuxCNC journal reports
`Probe tripped during a coordinate jog` at the manual contacts.

## Current effective contact height — 2026-09-13

The user selected **21.39 mm machine Z** as the plate-contact reference for the
installed tool, after correcting the observed contact reading to 21.405 mm.
The selection explicitly accounts for the button play and plate-contact
allowance described by the user. The retained slow IN0 trigger gives:

```text
effective_setter_height = fine_trigger_machine_z - accepted_plate_contact_machine_z
= 85.32116748046874 - 21.39
= 63.931167480468744 mm (stored f64)
```

[The current reference](../config/metrology/tool-setter.json) and
`[TOOL_SETTER] HEIGHT_ABOVE_PLATE_MM` now retain this effective trigger height.
[The original coarse and fine contacts](../config/metrology/tool-offsets/tool-setter-1789346395947653909-1102463.txt)
are archived unchanged, including the original trigger bits and 50 mm/min fine
feed. Future tool measurements subtract this height from their own normal
contact trigger; no additional play or pretravel term is subtracted. The
21.39 mm value belongs to this measured tool's plate reference, while the setter
height is the reusable calibration. No tool/work offset or machine command was
applied by recording this calibration.

## Programs using this reference

The three existing program paths and catalog IDs are retained. Their program
contents now use the BTER NC contact and overtravel circuits, with no clip or
OUT5 power cycle. The old 19.4 mm puck reference at machine X 288.125 / Y 152.955
and the `PUCK_*` INI entries remain historical calibration evidence only.

| Program | File | Sequence |
| --- | --- | --- |
| Tool Setter - Contact Check | [puck-contact-no-motion-test.ngc](../live/nc_files/puck-contact-no-motion-test.ngc) | No axis or spindle command. Observe an IN0 contact then release, with a 300-second timeout for each wait. |
| Tool Height - Slow Touch | [tool-height-first-test.ngc](../live/nc_files/tool-height-first-test.ngc) | One operator confirmation, downward Z touch at the original 6 mm/min, durable measurement, then the original Z-home return. |
| Tool Height - Double Touch | [tool-height-homing-style-test.ngc](../live/nc_files/tool-height-homing-style-test.ngc) | Start at machine Z home; confirm the manual pad press/release check, confirm measurement, touch at the original 300 mm/min, back off 1 mm above the trigger at 15 mm/min, re-touch at 15 mm/min, save the result, then return to Z home. |

Use the normal File Open and Run controls. The programs declare their effects,
prerequisites and Abort recovery to the typed script loader. Position the tool
manually above the recorded contact XY before a height measurement. The
existing 0.010 mm position guard remains; neither height program moves X or Y.
All probe descents are physical DOWN / LinuxCNC -Z; the existing backoff and
Z-home return are physical UP / LinuxCNC +Z. No probe feed or backoff value is
changed by this conversion. These original feeds are not a claim that the
manufacturer's rated repeatability has been established on the machine.

Both height programs call the same
[measurement implementation](../live/nc_files/dmc2_tool_setter_measure.ngc).
The shared `[TOOL_SETTER]` section in [dmc2.ini](../live/dmc2.ini) supplies the
accepted contact XY and height. The JSON reference and original recordings
retain the calibration evidence. Machine Z home and the downward search floor
come from the existing `[JOINT_2] HOME` and `[AXIS_Z] MIN_LIMIT` values.

The shared [input checks](../live/nc_files/dmc2_tool_setter_ready.ngc) select
normalized IN0 with P1 off, confirm selector feedback, and require both NC
circuits released with no retained overtravel or unavailable setter data. IN2
keeps its existing independent realtime stop and visible acknowledgement path.
The scripts never acknowledge overtravel or reset, enable or resume the machine.

### Coordinates and retained results

The measurement cancels the installed tool compensation with G49 as the old
programs did, snapshots the active work-to-machine translation in millimetres,
and converts both the Z search target and the original G38 trigger. It rejects
XY work-frame rotation. LinuxCNC defines `#5061..#5069` in the active work frame;
the [G38 documentation](https://linuxcnc.org/docs/2.9/html/gcode/g-code.html#gcode:g38)
describes the required conversion. No work or tool-table offset is written.
The saved modal state is restored on normal return, after the spindle has been
explicitly stopped; this cannot restore a previously running spindle.

M190 P7 starts a unique ledger in `tmp/output/tool-setter/`; P8 commits each
versioned record. The existing Rust capture path reads the original LinuxCNC
G38 trigger, retains its exact floating-point bits, synchronizes the file and
reads it back. An M66 barrier prevents subsequent backoff or return before that
step. The ledger stores the reference, work translation, requested feeds,
direction, each contact, the double-touch difference and the calculated result.

```text
machine_trigger_z = work_trigger_z + work_to_machine_z
plate_contact_machine_z = machine_trigger_z - setter_height_above_plate
tool_tip_height_at_home = machine_home_z - plate_contact_machine_z
```

The result uses the single slow touch or the final touch of the double sequence.
It reports the installed tool tip's height above the sampled plate reference at
machine Z home, not a newly applied tool-table length. Capture failure stops the
program before any following motion and reports `CAPTURE FAILED - KEEP THE
SETUP IN PLACE` with Abort and Pendant Mode recovery. Missing contact, failed
release and retained overtravel also stop with the applicable UI recovery action.

Build results, interpreter checks and retained synthetic data do not establish
physical tool-setting behavior; that requires a separately requested machine run.

## Plate-referenced Z tool offset

The M190 capture implementation snapshots the standard INI and accepted BTER
reference when a setter measurement begins. After the fine IN0 contact is
durably saved and read back, Rust derives and saves the installed tool's offset:

```text
plate_referenced_tool_offset_z = exact_fine_trigger_machine_z - accepted_setter_height
machine_z = desired_tool_tip_height_above_plate + plate_referenced_tool_offset_z
```

Each ledger has paired `.tool-offset.json` and `.tool-offset.ngc` files. The JSON
retains the exact trigger and its floating-point bits, calibration operands,
formula, normal-contact input, tool/setup scope, and whether the reference was
snapshotted at measurement start or explicitly supplied for an older recording.
The calibration files are saved alongside it. Existing files are preserved;
different output cannot silently replace an earlier result.

This is a plate-referenced Z compensation convention for the installed tool,
not a physical tool length measured from a spindle gauge line. It does not
identify a tool-table number or infer a flat plate plane from one sampled site.
It is the same `plate_contact_machine_z` reported by the older height programs.
Neither capture nor offline export executes an offset command.

The paired program can be opened through normal File Open and Run. It declares
coordinate-state effects and changes only Z tool compensation with
[G43.1](https://linuxcnc.org/docs/2.9/html/gcode/g-code.html#gcode:g43.1).
It requires the measured homed reference, a stopped spindle, G54 with zero Z
translation, and no active G52/G92 Z translation. XY work origins are independent.
It issues no axis travel. After explicit application, work Z0 represents the
accepted sampled plate reference for that installed tool. Machining programs
using a different part Z datum must include that datum translation once. Tool
replacement or a change in clamping requires the corresponding fresh measurement.
The script's recovery remains Abort then Pendant Mode; Clear Fault is never
disabled or conditional on offset availability.

For a historical recording without a reference snapshot, the installed Rust
binary accepts a data-only export with explicit calibration provenance:

```text
native/bin/dmc2-probe-capture --export-tool-offset <ledger> live/dmc2.ini config/metrology/tool-setter.json
```

Later exports of that ledger omit the reference arguments and read its saved
snapshot. Missing fine contact, invalid trigger bits, duplicate measurements, or
an INI height that differs from the accepted BTER reference reject the export.
