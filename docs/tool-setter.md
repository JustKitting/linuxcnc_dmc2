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

## Accepted tool-setter reference — 2026-09-13

The user accepted this setter as the new tool-setter system. Its reference is
stored in [tool-setter.json](../config/metrology/tool-setter.json), linked from
`live_requirements.json`. The accepted height above the sampled machine plate
is **63.995466247558595 mm**, using the measured height difference and nominal
zero setter pretravel.

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

### Historical programs and offsets

The old 19.4 mm puck reference at machine X 288.125 / Y 152.955 is superseded
for this setter and retained as historical data. Existing `PUCK_*` INI entries,
`puck-contact-no-motion-test.ngc`, `tool-height-first-test.ngc` and
`tool-height-homing-style-test.ngc` still describe the old puck and OUT5 sequence;
they do not consume the new reference. Recording this calibration applies no
tool-table or work-coordinate offset and changes no motion, wiring or recovery
behavior.
