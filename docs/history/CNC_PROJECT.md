# CNC control project

## USER-CONFIRMED motor command resolution — 2026-08-22

- All three motor DIP banks SW1..SW5 are `ON/OFF/ON/OFF/OFF`, selecting 4000
  command pulses per revolution.
- `/home/kit/cnc_motion_config.sh` is the single machine-wide setting used by
  reusable Bash and Python motion programs. It scales physical behavior from
  the previously tested 800-PPR configuration without rounding.
- The updated X/Y/Z home ran at 5000 pulses/second and backed each axis away
  250 pulses at 250 pulses/second. All expected limits triggered and cleared;
  the user confirmed the physical speeds and backoffs looked correct.
- Pendant detent requests remain 10/100/1000 pulses. Rates are 2500 for x1,
  15000 for x10, and the user-selected double x100 rate of 30000 pulses/second.
- Pendant limit recovery is 250 pulses away at 1500 pulses/second.
- The user accepted 1000 pulses/mm as the provisional X/Y/Z LinuxCNC scale for
  the rough operating-zone GUI. The rough caliper test passed its sanity check;
  fine measured-travel calibration remains later work.

## LinuxCNC single-owner migration baseline — 2026-08-22

### USER-OBSERVED ACTUAL architecture decision

- The Raspberry Pi is the LinuxCNC computer.
- The Arduino Nano attached to the pendant is a USB input passthrough/decoder,
  not a second LinuxCNC controller.
- The intended signal chain is pendant -> Nano -> USB -> LinuxCNC on the Pi,
  with that LinuxCNC instance communicating to the Mesa 7I95T over Ethernet.

### Implemented and validated offline; not activated on hardware

- The accepted provisional live profile is
  `live/dmc2.ini` in the main project.
- Normal software zones are exactly X 0..300 mm, Y 0..173 mm, and Z 0..135 mm
  at 1000 pulses/mm. The accepted switch coordinates are X 300.25, Y 173.25,
  and Z 135.25 mm; final homes are X 300, Y 173, and Z 135 mm.
- AXIS Home All is configured in X/Y/Z order. Each axis searches positive at
  5000 pulses/s, latches positive at 250 pulses/s, then finishes exactly 250
  pulses negative at 250 pulses/s.
- X/joint 0 remains stepgen 1/IN11, Y/joint 1 remains stepgen 0/IN9, and
  Z/joint 2 remains stepgen 2/IN10.
- The live pendant supervisor uses exact 10/100/1000-pulse increments and
  2500/15000/30000-pulse/s rates, side-button deadman control, one
  overwriteable latest request, the confirmed axis signs, exact -250-pulse
  limit bounce at 1500 pulses/s, and the confirmed E-stop recovery gesture.
- A realtime input latch, generated-count verification, controller heartbeat,
  and LinuxCNC E-stop chain fail closed on mismatched or stale state.
- `nano_hal_bridge.py` exposes read-only pendant status HAL pins and preserves
  the Nano's one-slot latest-detent behavior. It has no Mesa, motion, spindle,
  output, or probe-power command pin.
- `status_panel.xml` and its HAL fragments define AXIS/PyVCP visibility for
  X/Y/Z coordinates, generated pulse counts, live and latched limits, live and
  latched puck/probe contacts, OUT5 state, and pendant state.
- The Mesa status fragment forces OUT5 false and does not connect IN0 or IN1 to
  `motion.probe-input`.
- A hardware-free replay simulator and static/unit validation are present;
  the integrated suite passes 31 tests.
- A POSIX-realtime HAL smoke test loaded the replay bridge, status nets, and
  display latches without hardware. It reported the bridge connected with no
  serial fault; synthetic puck/probe pulses latched after their live signals
  cleared, and the display reset was also verified in the isolated HAL session.
- No LinuxCNC GUI, Mesa connection, Nano serial connection, motion, spindle,
  actuator, probe power, or output action was launched while building and
  validating this profile.

### Accepted provisional profile / explicit first-live boundary

- `live_requirements.json` separates the accepted
  provisional motion profile from explicitly deferred probing, alarm,
  physical-drive-enable, spindle, and integrated-hardware-validation work.
- `scripts/launch_live.py` defaults to validation only. The
  literal `--live` flag is required to start LinuxCNC, and that live path has
  not been run.
- OUT5 remains false, IN0/IN1 remain status-only, no spindle command is routed,
  no motor-alarm input is consumed, and no Mesa SSR controls the shared
  physical drive enable.

## Version control

This historical note predates the current project split. The main LinuxCNC
project and its sibling H100 Modbus project are now independent ordinary Git
repositories; the old home-directory bare repository and wrapper are archived.

## Current work boundary — 2026-08-22

- Spindle activation is paused at the user's instruction. The H100 work is
  preserved in the sibling `h100_modbus` project; its live run path had not been
  exercised.
- The current focus is tool-height and workpiece calibration using the puck and
  continuity probe.
- The confirmed puck/probe wiring, successful input test, and unresolved
  probing parameters are recorded in `docs/history/cnc_axis_mapping.md`.
- This state record does not authorize spindle activation, axis motion, probing
  motion, or wiring changes.
