# CNC axis and limit mapping

This record keeps physical observations separate from software telemetry.

## Loose X movement sanity check at 4000 PPR — 2026-08-22

- Starting caliper reading: 60.45 mm.
- Command: X / motor 1, right / negative, 50000 pulses at 5000
  pulses/second, with no return leg.
- Mesa-generated delta: exactly `-50000`; IN9, IN10, and IN11 remained clear.
- Final caliper reading: 110.14 mm.
- Measured travel: 49.69 mm.
- Calculated X scale: `50000 / 49.69 = 1006.23867981 pulses/mm`.
- Continuation command: X / motor 1, right / negative, 30000 pulses at 5000
  pulses/second, with no return leg.
- Continuation Mesa-generated delta: exactly `-30000`; IN9, IN10, and IN11
  remained clear.
- Second final caliper reading: 140.15 mm.
- Second-segment travel: `140.15 - 110.14 = 30.01 mm`, giving
  `30000 / 30.01 = 999.66677774 pulses/mm`.
- Combined travel: `140.15 - 60.45 = 79.70 mm` from exactly 80000 pulses,
  giving `80000 / 79.70 = 1003.76411543 pulses/mm`.
- USER-OBSERVED ACTUAL: these caliper readings were eyeballed and were not set
  up for fine calibration. Their spread must not be characterized as backlash.
- These results are retained only as a coarse movement sanity check. The
  preliminary LinuxCNC/GUI scale remains the nominal `1000 pulses/mm`; final
  per-axis calibration is explicitly deferred until the GUI is running and a
  fine measurement setup is available.

## Separate home and calibration commands — confirmed requirements

- `home` and `calibrate` are separate commands.
- `home` approaches the mapped switches in X, Y, Z order at 5,000 pulses per
  second, then targets a 250-pulse reverse backoff at 250 pulses per second.
- A home backoff from 245 through 255 generated pulses is accepted. The actual
  generated backoff is always reported, and the corresponding switch must clear.
- `calibrate` starts from the offset home position and slow-walks back toward the
  end switches.
- Calibration must retry the last unsuccessful axis sequence once before it
  declares a fault. The calibration command has not yet been run.

## Motor command resolution and confirmed 4000-PPR home — 2026-08-22

- USER-OBSERVED ACTUAL: all three motor DIP banks SW1..SW5 are
  `ON/OFF/ON/OFF/OFF`.
- The reusable machine-wide setting is
  `/home/kit/cnc_motion_config.sh`: `MOTOR_PULSES_PER_REV=4000`, referenced
  against the previously tested 800-PPR configuration.
- The nominal 4 mm screw lead gives 1000 pulses/mm. Exact per-axis scale still
  requires longer measured-travel calibration.
- The scaled home ran in X, Y, Z order at 5000 pulses/second with a 250-pulse
  backoff at 250 pulses/second.
- X / motor 1: `+20264` to IN11, then `-251`; IN11 cleared.
- Y / motor 0: `+215` to IN9, then `-252`; IN9 cleared.
- Z / motor 2: `+259` to IN10, then `-253`; IN10 cleared.
- All limits were clear at completion, the program exited normally, and the
  user confirmed that the physical approach and backoff speeds looked correct.

## Motor-driver alarm monitoring — future requirement noted 2026-08-21

### USER-OBSERVED ACTUAL

- The X-axis failed to move while the pendant controller was accepting commands;
  the user identified an active motor-driver alarm as the cause.

### PROPOSAL — NOT ACTUAL

- Wire each motor driver's alarm output into an available Mesa input so control,
  homing, calibration, and probing programs can monitor driver alarms.
- A detected alarm must be reported with its affected motor/axis and treated as
  a motion fault instead of allowing commanded counts to continue unnoticed.
- Exact Mesa input assignments, alarm-output polarity, shared/common wiring,
  debounce behavior, and reset policy remain undetermined. This note does not
  authorize wiring changes or assign any terminal.

## Probe and puck calibration I/O — verified 2026-08-21

### USER-OBSERVED ACTUAL wiring

- Encoder 5 `+5V` is wired to the topmost terminal of the TB5 `OUTPUTS 0..5`
  section, identified as `OUT5+`.
- `OUT5-` is wired to the shared `INCOM0,1` terminal.
- The puck is wired to `INPUT0`.
- The DMC2 continuity/resistance probe is wired to `INPUT1`.
- The alligator clip is the ground side of both contact circuits and is wired
  to Encoder 5 ground.
- No series current-limiting resistor was confirmed in the final user-stated
  wiring. Future documentation and code must not claim that one is installed
  unless the user explicitly confirms it.

The resulting user-confirmed topology is:

```text
TB2 pin 22  Encoder 5 +5V  -> TB5 pin 24 OUT5+
TB5 pin 23  OUT5-          -> TB6 pin 3  INCOM0,1
TB6 pin 1   INPUT0         -> puck
TB6 pin 2   INPUT1         -> DMC2 probe
TB2 pin 19  Encoder 5 GND  -> alligator clip
```

### SOURCE-VERIFIED mapping

- Official Mesa 7I95T manual:
  <https://www.mesanet.com/pdf/parallel/7i95tman.pdf>
- TB2 pin 19 is Encoder 5 `GND FROM 7I95T`; TB2 pin 22 is Encoder 5
  `+5V FROM 7I95T` (PDF page 13 / printed page 10).
- TB5 pin 23 is `OUT5-`; TB5 pin 24 is `OUT5+` (PDF page 16 / printed page
  13). The board-layout drawing identifies pin 1, making pin 24 the topmost
  terminal in this installed orientation.
- TB6 pin 1 is `INPUT0`; TB6 pin 2 is `INPUT1`; TB6 pin 3 is `INCOM0,1`
  (PDF page 17 / printed page 14).
- The isolated outputs are polarity-sensitive floating MOSFET switches. The
  isolated inputs contain 4.7 kOhm series resistance. For ground-contact/NPN
  operation, the paired input common is connected to `+5V` through `+36V`
  (PDF pages 20–21 / printed pages 17–18).

### Live software mapping and positive test

- OUT5 command: `hm2_7i95.0.ssr.00.out-05`
- INPUT0 raw/filtered:
  `hm2_7i95.0.inmux.00.raw-input-00` and
  `hm2_7i95.0.inmux.00.input-00`
- INPUT1 raw/filtered:
  `hm2_7i95.0.inmux.00.raw-input-01` and
  `hm2_7i95.0.inmux.00.input-01`
- Test program: `/home/kit/probe_io_test.py`
- Continuous capture:
  `/home/kit/probe_io_capture_20260821_041525.tsv`
- The isolated test loaded zero step generators, asserted only OUT5, and
  captured 81,778 samples at approximately 1 kHz over 81.838 seconds.
- INPUT0 was detected by both raw and filtered signals. Its first high interval
  lasted about 38 ms and included contact bounce: four raw and four filtered
  transitions were recorded.
- INPUT1 was detected by both raw and filtered signals. Its high interval
  lasted about 23 ms: two raw and two filtered transitions were recorded.
- OUT5 was read back low before teardown, and the isolated HAL session fully
  unloaded. No motion channels were configured or commanded.

### Confirmed requirement for the future calibration script

- OUT5 provides probe-circuit power programmatically and is to be enabled only
  while an explicit probing/calibration operation is active.
- The future script must use the mappings above and must detect short contact
  events; it cannot depend on a long user-held contact.
- The exact probing motion sequence, axis, direction, pulse rate, maximum
  travel, retract, coordinate offset, and ordering have not yet been specified
  or authorized. Nothing in this record authorizes probe motion.

## Successful offset-home run — 2026-08-20

- Script: `/home/kit/xyz_limit_backoff_test.sh`
- Order: X, Y, Z.
- Approach rate: 1,000 pulses per second.
- Backoff target: 50 pulses at 50 pulses per second; accepted window 45–55.
- X / motor 1: `+5052` to IN11, then `-50`; IN11 cleared.
- Y / motor 0: `+10546` to IN9, then `-51`; IN9 cleared.
- Z / motor 2: `+12064` to IN10, then `-51`; IN10 cleared.
- Final state reported by the board: IN9, IN10, and IN11 all clear.
- Sequence completed normally; HAL then exited.

| Motor | User-observed `+` direction | User-observed `-` direction | Limit approached | Latched input | Round-trip generated counts | Physical return |
|---|---|---|---|---|---|---|
| 0 | Forwards | Backwards | Forwards | IN9 | `+9519`, then `-9519`; net `0` | User confirmed |
| 1 | Left | Right | Left | IN11 | `+18678`, then `-18678`; net `0` | User confirmed |
| 2 | Up | Down | Up | IN10; manual and motion-trigger confirmed | `+12039`, then `-12040`; net `-1` | User confirmed |

## Z-axis limit round trip — run 2026-08-20

- Motor: 2
- Approach: `+` / up at 10 pulses per second
- Stop condition: first latched input among IN9, IN10, and IN11
- Return: `-` / down by the exact generated approach count at 100 pulses per second
- Script: `/home/kit/motor2_limit_roundtrip.sh`
- Actual stop input: IN10.
- Upward generated delta: `+12039`.
- Return generated delta: `-12040` against target `12039`.
- Net generated count: `-1`.
- Physical return observation: user confirmed return to the original position.

## Z-axis wiring status — 2026-08-19

- User-observed actual: the Z-axis wiring was updated.
- User assessment: the Z-axis issue may now be fixed.
- Verification status: the rewired Z sensor was manually triggered and detected
  on both `hm2_7i95.0.inmux.00.raw-input-10` and filtered `input-10`.
- Realtime IN10 latch fired; IN9 and IN11 remained inactive.
- Continuous 1 ms capture recorded two IN10-high intervals of 111 samples and
  380 samples. This is positive detection, not a shell-polling coincidence.
- Capture: `/home/kit/z_sensor_capture_20260819_220926/in9_in10_in11.tsv`
- No motion was sent during this verification.
