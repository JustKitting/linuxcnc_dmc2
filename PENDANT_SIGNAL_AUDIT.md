# DMC2 pendant exhaustive signal and command audit

This is the mandatory pre-hardware-test checklist for the MYST1474-001 pendant,
Arduino Nano P3 decoder, USB bridge, LinuxCNC supervisor, Mesa limit gate, and
AXIS status panel. A checked box means that row passed both named, independent,
hardware-free paths after the final source change. Nothing in either pass opens
the Nano serial device, LinuxCNC NML, or the Mesa card, and neither pass can
move hardware.

## Fixed user-observed mapping under test

| Physical signal | Nano/P3 representation | LinuxCNC result | Pass 1 | Pass 2 |
|---|---|---|---|---|
| Wheel A, green | D2 / quadrature A | interrupt input only | [x] | [x] |
| Wheel B, white | D3 / quadrature B | interrupt input only | [x] | [x] |
| Axis X | D4 / `X` | X axis, motor 1 | [x] | [x] |
| Axis Y | D5 / `Y` | Y axis, motor 0 | [x] | [x] |
| Axis Z | D6 / `Z` | Z axis, motor 2 | [x] | [x] |
| Axis 4 | D7 / `4` | visible, no CNC motion | [x] | [x] |
| Axis 5 | D8 / `5` | visible, no CNC motion | [x] | [x] |
| Axis 6 / no decoded pair | `N` as physically observed | no CNC motion | [x] | [x] |
| Multiplier x1 | D9 / `X1` | 10 pulses per detent | [x] | [x] |
| Multiplier x10 | D10 / `X10` | 100 pulses per detent | [x] | [x] |
| Multiplier x100 | D11 / `X100` | 1000 pulses per detent | [x] | [x] |
| E-stop released | D12 LOW / `0` | permits normal state processing | [x] | [x] |
| E-stop pressed or open | D12 HIGH / `1` | stop, gate false, recovery required | [x] | [x] |
| Side button released | `deadman=0` | cancel/deny jog | [x] | [x] |
| Side button held | `deadman=1` | jog only after a fresh matching baseline | [x] | [x] |
| Clockwise | `latest-detent=+1` | X right; Y forward; Z up | [x] | [x] |
| Counterclockwise | `latest-detent=-1` | exact inverse of clockwise | [x] | [x] |
| No complete detent | `latest-detent=0` | no new jog | [x] | [x] |

## Complete P3 state space

Both passes enumerate all `7 axes × 5 multipliers × 2 deadman states × 2
E-stop states × 2 selector-valid states × 3 detent signals = 840` possible P3
packet-state combinations. Exactly 18 combinations may produce a command:
`3 configured axes × 3 configured multipliers × 2 directions`, with E-stop
released, selector valid, and deadman held. Every other combination must stop
or remain non-commanding.

| Check | Pass 1: direct interpreter | Pass 2: raw P3 → bridge → HAL snapshot → supervisor |
|---|---|---|
| All 840 states enumerated | [x] | [x] |
| Exactly 18 command combinations | [x] | [x] |
| Exact axis-to-motor mapping | [x] | [x] |
| Exact clockwise/counterclockwise signs | [x] | [x] |
| Exact 10/100/1000-pulse distances | [x] | [x] |
| Homed teleop command mode | [x] | [x] |
| Unhomed joint-jog command mode | [x] | [x] |
| Axis 4, 5, OFF/6, and invalid never move | [x] | [x] |
| Multiplier OFF and invalid never move | [x] | [x] |
| Released deadman, E-stop, or invalid selector never moves | [x] | [x] |

## Rate policy

The requested change scales each preceding rate independently. It does not
change distance, acceleration, the exact limit bounce, or the separately
accepted LinuxCNC 30 mm/s machine ceiling.

| Setting | Previous | Multiplier | Requested command rate | Pass 1 | Pass 2 |
|---|---:|---:|---:|---|---|
| x1 | 2500 pulses/s | ×2 | 5000 pulses/s (5 mm/s request) | [x] | [x] |
| x10 | 15000 pulses/s | ×5 | 75000 pulses/s (75 mm/s request) | [x] | [x] |
| x100 | 30000 pulses/s | ×10 | 300000 pulses/s (300 mm/s request) | [x] | [x] |
| Limit bounce | 1500 pulses/s | unchanged | 1500 pulses/s, exactly -250 pulses | [x] | [x] |
| LinuxCNC ceiling | 30 mm/s | unchanged | planner clamps requests to 30 mm/s | [x] | [x] |

## Firmware transition integrity

| Boundary/error | Required behavior | Pass 1 | Pass 2 |
|---|---|---|---|
| Selector changes | publish invalid immediately; require 20 ms stable state | [x] | [x] |
| Deadman press/release | same selector transition rule; clear pending wheel fragment | [x] | [x] |
| E-stop press/release | clear pending wheel fragment before publishing edge | [x] | [x] |
| Partial quadrature across any boundary | discarded; never becomes a later command | [x] | [x] |
| CW sequence `00→10→11→01→00` | one `+1` detent | [x] | [x] |
| CCW inverse sequence | one `-1` detent | [x] | [x] |
| Illegal diagonal quadrature edge | increments error count; link becomes faulted | [x] | [x] |
| One-slot report behavior | at most `-1`, `0`, or `+1`; never replays count delta | [x] | [x] |
| Worst-case P3 line at 115200 baud | fits inside one 20 ms report interval | [x] | [x] |
| Final Nano source | compiles for the installed classic Nano target | [x] | [x] |

## Link and protocol lifecycle

| Event | Required behavior | Pass 1 | Pass 2 |
|---|---|---|---|
| Boot marker | fresh fail-closed baseline | [x] | [x] |
| First valid packet | connected, but detent discarded | [x] | [x] |
| Normal next packet | exposes only its one-slot detent | [x] | [x] |
| Sequence gap | count dropped packets; do not reconstruct movement | [x] | [x] |
| Duplicate/reverse sequence | protocol fault and fresh baseline required | [x] | [x] |
| Unsigned 32-bit sequence wrap | accepted in order | [x] | [x] |
| Malformed/old/overlong/out-of-range packet | protocol fault; no command | [x] | [x] |
| Quadrature error-count change | sticky fault; no command | [x] | [x] |
| 100 ms packet timeout | serial fault; no command | [x] | [x] |
| Reconnect after fault/timeout | first packet is non-commanding baseline | [x] | [x] |
| Diagnostic counters beyond HAL u32 | explicitly masked; publisher cannot crash | [x] | [x] |

## Motion readiness, queue, limits, and recovery

| Case | Required behavior | Pass 1 | Pass 2 |
|---|---|---|---|
| Machine off, E-stopped, non-manual, interpreter running, or homing | deny/cancel pendant jog | [x] | [x] |
| Homed + manual + teleop + idle | finite axis jog allowed | [x] | [x] |
| Unhomed + manual + joint mode + idle | finite joint jog allowed | [x] | [x] |
| Same axis and direction | replace target from generated count | [x] | [x] |
| Axis or direction changes | stop once; keep one overwriteable pending request | [x] | [x] |
| 100,000 synthetic detents | no crash and no unbounded pending queue | [x] | [x] |
| Own positive limit for X/Y/Z | realtime stop, then exact -250 pulse bounce | [x] | [x] |
| Moving away, wrong limit, or multiple limits | fail closed; no invented bounce | [x] | [x] |
| Startup with one matching active limit | use the same exact bounce path | [x] | [x] |
| Bounce -249/-251, timeout, or raw input still active | fail closed | [x] | [x] |
| Homing limit event | homing owns it; latch clears only after raw clears | [x] | [x] |
| Exact E-stop recovery gesture | input-only until the third complete click | [x] | [x] |
| Any wrong recovery transition or active limit | restart/refuse recovery | [x] | [x] |

## AXIS GUI and notification integrity

| Check | Required behavior | Pass 1 | Pass 2 |
|---|---|---|---|
| XML and post-GUI HAL | every panel pin exists exactly once and is wired | [x] | [x] |
| Compact layout | coordinate/pulse and limit/contact frames are side by side | [x] | [x] |
| Typography | every operator label and numeric field uses one Helvetica system | [x] | [x] |
| Text alignment | box text is left justified | [x] | [x] |
| Selector alignment | X/X1, Y/X10, and Z/X100 share vertical columns | [x] | [x] |
| Physical selector display | no internal Invalid state or impossible scale OFF lamp | [x] | [x] |
| Top-level fail-closed state | control-ready and fault are the first status frame | [x] | [x] |
| Home button | momentary route to existing `halui.home-all` X/Y/Z sequence | [x] | [x] |
| Input columns | Limits and Contacts use identical Input/LIVE/SEEN columns | [x] | [x] |
| Clear button | compact; clears display-history latches only | [x] | [x] |
| Expected `Jog aborted by jog-stop-immediate` | retained in logs, hidden from AXIS popup | [x] | [x] |
| Every other NML/operator/info message | remains visible | [x] | [x] |
| Visible notifications | anchored away from the right-side status panel | [x] | [x] |
| Rendered panel | no overlap, clipping, or uncontrolled expansion at 1920×1080 | [x] | [x] |

## Evidence and research basis

- User-observed live mapping: `pendant_nano/LIVE_MAPPING.md`.
- Mesa/limit policy: `live/machine.hal`, the compiled
  `rust/crates/dmc2-rt` servo-thread component, and the exact existing
  -250-pulse bounce tests.
- LinuxCNC `motion.jog-stop-immediate` semantics and the source-level
  `reportError` behavior: official LinuxCNC `motion(9)` documentation and
  `src/emc/motion/control.c` at official tag `v2.9.10`.
- LinuxCNC incremental jog API: `halui.axis.*.increment`,
  `increment-plus`, `increment-minus`, and `jog-speed` as implemented in
  `src/emc/usr_intf/halui.cc` at official tag `v2.9.10`.
- PyVCP layout behavior: installed LinuxCNC 2.9.10 `pyvcp_widgets.py` plus the
  official PyVCP documentation.
- Nano interrupt/pin basis: official Arduino Nano documentation and the
  ATmega328P datasheet (INT0/INT1 on the D2/D3 interrupt mapping used by the
  Arduino core).

## Final run record

| Run | Command group | Result | Timestamp |
|---|---|---|---|
| Independent pass 1 | 15 Nano + 28 control + 72 LinuxCNC tests; validator; two dry CLIs | [x] | 2026-08-22T23:46:00-04:00 |
| Independent pass 2 | Same 115 tests in fresh processes with `PYTHONHASHSEED=271828`; validator; two dry CLIs | [x] | 2026-08-22T23:48:00-04:00 |
| Firmware compile 1 | Fresh classic Nano build: 4290-byte flash, 265-byte RAM | [x] | 2026-08-22T23:46:00-04:00 |
| Firmware compile 2 | Separate fresh classic Nano build: identical flash/RAM result | [x] | 2026-08-22T23:48:00-04:00 |
| GUI render 1 | `/tmp/dmc2-layout-preview-pass1.png`, actual 1920×1080 desktop | [x] | 2026-08-22T23:38:00-04:00 |
| GUI render 2 | `/tmp/dmc2-layout-preview-pass2.png`, final 48-pin panel | [x] | 2026-08-22T23:47:00-04:00 |
| GUI render 3 | `/tmp/dmc2-layout-preview-pass3.png`, revised 42-pin panel at the live 540×680 size | [x] | 2026-08-23T07:48:00-04:00 |
