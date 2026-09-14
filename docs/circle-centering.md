# Internal circle centering

[find-circle-center.ngc](../live/nc_files/find-circle-center.ngc) uses the
standard DMC2 typed file loader. Its capture gate is the compiled Rust
`native/bin/dmc2-probe-capture`, reached through the `live/nc_files/M190`
symlink. There is no Python machine-control path or alternate launcher.

## Settings and entry

Open **Custom Scripts → Hole centering**, enter **Max travel / direction (mm)**,
then press **Run Hole centering**. The +/− controls change the field by 1 mm;
typing permits decimal values. The initial value is the operator's requested
25 mm. AXIS preferences retain valid edits across sessions. The field is the
maximum distance of each contact approach from that pass's center, in each
signed axis direction; it is not a diameter. The reusable source reads
`axisui.circle-search-mm` at entry; no per-run file is edited or generated.
Invalid editing text clears `axisui.circle-parameters-valid`, so neither the
pane nor a direct File Open/Run can silently use the preceding valid value.
Initial search contact uses `coarse_feed_mm_min = 800`, the operator-requested
4× increase from 200 mm/min. The final measurement, release, return, and centering
feed stays at `feed_mm_min = 50`. The search-feed check accepts the requested
800 mm/min maximum separately from the published **50–200 mm/min** probe feed
range. The final measurement uses the published lower endpoint, not a separately
manufacturer-designated optimum. Sources:
[Amazon specification](https://www.amazon.com/dp/B0CC5BFFZW) and
[manufacturer Main Specs](https://pgfuntransmission.com/product/npn-nc-cnc-3d-touch-probe-with-6-mm-shank-and-2-0-mm-tungsten-steel-ball-tip/).
No separately guessed backoff distance is used.

The ball starts clear inside the opening at the intended probing Z. The script
makes no Z movement and writes no work or tool offsets. Its checks require all
axes homed, machine on, E-stop clear, an idle interpreter, spindle off, and no
active work-frame XY rotation. Every search endpoint must lie strictly inside
the configured X/Y travel limits; endpoints are never silently clipped or
extended. Nominal ball diameter is 2.0 mm, from the probe's product specification
linked in [the plate reference](known-plate-position.md).

Build with `scripts/build_native.sh`. LinuxCNC discovers user M codes at task
initialization, so **the new M190 becomes available on the next standard session
launch/restart**. This implementation does not restart the current session.
An unavailable M190 prevents program execution rather than bypassing capture.
The pane shows unmet machine prerequisites before Run; it does not home or
clear faults automatically. Parameter fields are held during an explicitly
submitted run and unlock after LinuxCNC reports it stopped. The existing
Abort, Clear Fault and Pendant Mode controls remain outside that field lock.

## Measurement sequence

Each pass approaches Y-, returns to the pass center, approaches Y+, returns,
approaches **physical RIGHT / LinuxCNC -X**, returns **physical LEFT / LinuxCNC
+X**, approaches **physical LEFT / LinuxCNC +X**, and returns **physical RIGHT /
LinuxCNC -X**. Every return uses the same axis as its approach. When contact is
still asserted, a release probe stops on loss of contact. Each wall receives
a coarse contact, release backoff, slow re-touch, release, then return to the
already-clear pass center. Both trigger records are saved before their respective
release moves; **only the slow trigger enters the circle calculation**.
A new center move is a
contact-sensitive XY move: unexpected contact aborts instead of being used as a
normal wall measurement. For a center move, increasing X means **physical LEFT /
LinuxCNC +X** and decreasing X means **physical RIGHT / LinuxCNC -X**.

The script copies `#5061..#5063` immediately after a successful G38 contact,
before any release move overwrites them. They are **work-frame** coordinates.
With rotation rejected, the retained start offsets
`offset_axis = #<_abs_axis> - #<_axis>` translate these into machine XYZ,
including the tool offset already in effect. They do not establish a physical
ball-center Z calibration.

Every staged record is followed by M190 and a queue-busting M66 input read.
The helper validates the sequence and required fields, reads the original
`emcStatus.motion.traj.probedPosition`, checks its agreement with the staged
machine-frame trigger, saves the original floating-point values and bit patterns,
flushes them to storage, and reads the saved bytes back. Only a successful helper
exit allows the interpreter past that barrier. It never substitutes stopped
positions. The nine-place script serialization is checked against the original
coordinates using its rounding allowance plus floating-point arithmetic error;
this is not a machining tolerance.

Missing contact, invalid geometry, stale/malformed capture, failed write, failed
sync, or failed readback stops the program with the actual condition and the
**Abort → Pendant Mode** recovery path. Capture errors state **CAPTURE FAILED —
KEEP THE SETUP IN PLACE**. Clear Fault remains the existing visible route for a
latched machine fault. The existing `dmc2_abort.ngc` clears program probe selection.
No automatic retry, restart, homing, fault clearing, or recovery movement is added.

## Center, shape, and repeated positions

For a pass at `(x0,y0)` with contacts `xm,xp,ym,yp`:

```text
DX = xp - xm                   DY = yp - ym
cx = (xp + xm) / 2              cy = (yp + ym) / 2
span_error = abs(DX - DY)
midpoint_residual = hypot(cx - x0, cy - y0)
radius_from_X = sqrt((DX / 2)^2 + (cy - y0)^2)
radius_from_Y = sqrt((DY / 2)^2 + (cx - x0)^2)
nominal_bore_diameter = radius_from_X + radius_from_Y + ball_diameter
```

Opposite midpoints cancel the same nominal ball radius. The radius estimates
describe the ball-center locus, so the bore diameter includes the ball diameter.
Both radius estimates and both spans are retained to expose disagreement with a
circular cross-section. Four contacts do not establish the shape between them.

Matching chord lengths alone does not establish a center: a diagonal offset can
give equal lengths. The next candidate comes from the opposite midpoints. It is
rounded to the **command increment** read from each joint's SCALE, anchored at
the initial commanded position so its fractional origin is preserved. This
handles an intermediate half-step without claiming that configured command
resolution guarantees physical positioning accuracy.

After moving to a candidate, the script measures again. Even an initially
centered position receives a confirmation pass. Visited positions are compared
as integer command-grid indices. On revisiting a prediction, it returns to the
visited position with the user's requested **smallest `abs(DX-DY)`**; ties prefer
the smaller midpoint residual. That score is also retained when it is nonzero.
The selected position is not silently relabeled a true center merely because it
has the best span match. A distorted opening can favor a span match away from
its opposite-midpoint center; the retained residual exposes that distinction.

The routine reserves volatile user parameters **4000–4999**, currently unused by
the other project programs, for pairs of visited grid indices. Their capacity is
a storage limit, not a universal retry count. It does not write persistent work
offset parameters. On exhausting this storage it returns to the best measured
candidate and explicitly reports that convergence was not established.

| Result reason | Meaning |
| --- | --- |
| 1 | Latest opposite midpoint falls within half of the current command increment on each axis, and span mismatch is within the sum of the axis command increments. The selected best score and its own residual remain explicit. |
| 2 | The next predicted command position has already been measured; return the best observed span match. |
| 3 | Visit storage exhausted; return the best observed span match without claiming convergence. |

The half-increment and sum-of-increments comparisons follow the command grid.
They are not a sensor repeatability or physical accuracy claim.

## Retained output

Each run gets a unique `tmp/output/circle/circle-<timestamp>-<pid>.txt` ledger.
Each record has a sequence and `BEGIN`/`END` delimiters. Records are typed
`start`, `touch`, `sweep`, `selection`, and `result`. Touch axis codes are X=0,
Y=1; direction is the signed LinuxCNC direction. Version 2 records distinguish
stage 0 (coarse location) from stage 1 (fine measurement), and retain each feed.
All motion metadata uses mm and
commanded mm/min; it is not a measurement of actual instantaneous velocity.

`selection` is the provisional best measured target before the final move.
`result` is written only after LinuxCNC reports that return finished and program
probe selection cleared. A ledger ending at `selection`, a partial record, or an
error is not a completed centering run. User-observed mismatch or a machine fault
invalidates a conflicting result. No output is a claim of independently measured
physical arrival.

Result `x,y` are active work-frame coordinates; `machine_x,machine_y` explicitly
include the recorded work-to-machine translation for later models and scripts.
The original trigger coordinates additionally retain exact f64 bit patterns.
Previous run ledgers are never truncated. `circle-request.txt` and
`circle-active.txt` are internal transaction state, not final results.

The reusable plate reference is separate at
[`config/metrology/known-plate-position.json`](../config/metrology/known-plate-position.json).
This circle routine does not overwrite those plate measurements.

## Implementation checks so far

The Rust capture tests exercise malformed/stale/nonfinite records, consuming a
record only once, saved-byte readback, exact-bit retention, and rejection of a
different probe coordinate. The standalone LinuxCNC interpreter has exercised
the program arithmetic with synthetic off-center, equal-chord diagonal,
initially centered, half-command-step, ellipse/repeat, and missing-contact
fixtures. These are assistant-arranged checks, not evidence of machine behavior.
The two-speed fixture deliberately gave coarse contacts different coordinates;
the retained feeds/stages and fine-only circle spans matched the check assertions.
The circle routine has not been run on the physical machine.
