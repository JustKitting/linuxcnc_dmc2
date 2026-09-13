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

## Calibration boundary

The stored legacy puck height and location are not measurements of this setter.
This configuration does not change or establish tool-setter height, position,
contact travel, overtravel distance or a tool offset. Existing tool-height
programs still contain their prior calibration and are not run by this change.
