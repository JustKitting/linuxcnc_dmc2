# Shallow top surface mapping

[`map-top-surface.ngc`](../live/nc_files/map-top-surface.ngc) is a parameterized
script for **File → Open** through the existing typed DMC2 loader. It uses the
standard compiled Rust M190 capture helper, also used by the circle routine.
There is no new launcher, Python motion path, service, recovery latch, automatic
restart or automatic fault clear. The file is an implementation proposal; it
has not been run on the machine.

## Region and reference

The operator starts with the ball **clear just above the chosen high reference
point**, with the spindle off. That point need not be a corner of the rectangle.
`x_min_offset_mm`, `x_max_offset_mm`, `y_min_offset_mm`, and `y_max_offset_mm`
define a rectangle relative to that starting XY, in signed LinuxCNC coordinates.
Positive X is **physical LEFT / LinuxCNC +X**; negative X is **physical RIGHT /
LinuxCNC -X**. `spacing_mm` is the grid interval. The final interval on an axis
may be shorter to include the exact region endpoint; the region is never enlarged.

The region, grid spacing and mounted usable reach are initially **unset**. Run
rejects the file before motion until they are entered. No region or resolution
has been inferred from the previously recorded plate edge. The existing plate
reference does not yet establish the complete plate boundary or this object's
extent.

After a coarse reference contact, release and slow re-touch, the slow trigger
sets the relative height origin. The proposed settings are:

| Setting | Value and meaning |
| --- | --- |
| `reference_search_mm` | 3 mm maximum initial downward search from the entry position |
| `max_drop_mm` | 3 mm maximum depth below the measured reference for every grid point |
| `clearance_mm` | 2 mm above the measured reference for every XY transfer |
| `reach_reserve_mm` | 2 mm retained from the mounted usable reach |
| `usable_reach_mm` | Unset; clearance from ball bottom to the first part that could interfere over this region |
| `coarse_feed_mm_min` | 200 mm/min for location and contact-sensitive clearance travel |
| `fine_feed_mm_min` | 50 mm/min for measurement and release |

The depth check is `requested drop <= usable reach - reserve`; the initial
reference search is checked against the same budget. Exceeding it stops before
motion instead of clipping a requested distance. Clearance and depth are
separate: with the proposed settings a dip from the clearance plane to the
search floor spans 5 mm, of which 3 mm is below the reference.

The manufacturer [dimension drawing](https://pgfuntransmission.com/wp-content/uploads/2024/11/6s-Nc.jpg)
shows a **22 mm new stylus** measured from its seating shoulder to ball bottom,
and **26 mm from body underside to ball bottom for the old stylus assembly**.
Those are different dimension references. They do not establish this probe's
installed clearance to its taper, collar, body or surrounding features, so they
are not silently assigned to `usable_reach_mm`.
The [product specification](https://www.amazon.com/dp/B0CC5BFFZW) gives a nominal
2 mm ball, feed range 50–200 mm/min and Z overtravel of 2 mm. Mechanical overtravel
after contact is not an available depth to search through empty space.
No separately designated optimum feed was found; the lower published endpoint
is the proposed final measuring speed. This gives a 4× coarse/fine ratio.

The reference-plus-clearance plane must clear everything traversed, including
the entry path and probe assembly. This is a setup prerequisite, not a claim
that a single top contact has discovered every higher object. Probing Z, the
full XY region and possible reference/clearance/depth envelope are checked
against the configured machine limits. Rotated XY work frames are rejected.
Work and tool offsets are neither changed nor recalibrated.
The reported pre-transfer Z is compared with the clearance target within half
the configured Z command increment from JOINT_2 SCALE. This avoids requiring
bit-identical floating-point coordinates after LinuxCNC refreshes position;
the commanded clearance target is unchanged. This is not a physical accuracy
claim or a tolerance applied to the retained trigger measurements.

## Sequence and missing points

1. Capture both reference triggers, saving and reading each back before release.
2. Retract in Z to the common clearance plane.
3. Traverse the grid in alternating X directions, advancing rows in LinuxCNC +Y.
   Even rows move **physical LEFT / LinuxCNC +X**; odd rows move **physical RIGHT /
   LinuxCNC -X**. Entry X follows those same signed mappings.
4. At each point probe downward to the fixed floor at the coarse feed. If it
   touches, save the trigger, back off until release, re-touch slowly, save that
   trigger, then release and retract. Only the slow trigger becomes a map height.
5. If the coarse search reaches the floor without contact, save an **unmeasured**
   point and its search boundary, then retract. Do not extend the search.
6. After the last point, remain there at clearance, clear program probe selection,
   retain a result record, and export the map.

The floor is always `reference trigger Z - max_drop_mm`. It does not descend
from each preceding sample. Both retractions after release and XY transfers
are contact-sensitive. An unexpected transfer contact is captured durably as
an obstruction, then the program stops in place; it is not used as a top sample.
A failed initial reference or failed slow re-touch is a capture failure, not a
normal unmeasured grid point. No retry, return to origin or recovery motion is
added after an error.

## Retained map and coordinate meaning

Each run creates a unique `tmp/output/surface/surface-<timestamp>-<pid>.txt`
ledger. Each successful contact retains:

- The exact original `emcStatus.motion.traj.probedPosition` machine XYZ values
  and their floating-point bit patterns, checked against the staged G38 trigger.
- Work coordinates, translation to machine coordinates, commanded feed, stage,
  point identifier, grid row/column and downward LinuxCNC Z direction.
- The sequence and record boundaries, flushed to disk and read back before any
  release or return. A stopped position never substitutes for a trigger.

After a normal result record the helper writes a CSV and JSON with the same
basename, saves them and reads them back. CSV rows distinguish `measured` from
`unmeasured`. Unmeasured rows have **empty height cells**, not zero or the search
floor. Fine contact coordinates are exported with their retained precision.
Relative Z is exactly `fine machine trigger Z - fine reference machine trigger Z`.
The JSON retains settings, reference operands, coordinate frame, expected and
recorded counts, and whether a program result or obstruction was recorded.

This is the **top contact envelope of the mounted spherical probe**. It can
support a sampled height map and locating top features in the machine frame.
It does not recover overhangs, vertical walls, unsampled narrow features or the
surface below a missed point. It applies no automatic ball-radius subtraction
on slopes and no unmeasured spindle-to-ball correction. Absolute physical surface
Z remains uncalibrated; the unknown constant mounting length cancels in relative
Z while the same probe mounting and frame are retained.
The earlier [`known-plate-position.json`](../config/metrology/known-plate-position.json)
is linked as a separate reference. Its data is preserved; no complete CAD
alignment or legacy side-probe offset is silently applied.

A stopped run retains earlier records. Their offline export is available without
connecting to or commanding LinuxCNC:

```sh
native/bin/dmc2-probe-capture --export-surface tmp/output/surface/<retained-ledger>.txt
```

An export without a result is explicitly provisional in its JSON. An incomplete
ledger record or inconsistent counts is rejected. Previous ledgers and different
existing exports are not overwritten. No assistant-only command is needed to
regain machine control; offline export is only data processing.

## Errors, build and current evidence

Errors identify the condition and **Abort → Pendant Mode** recovery. Clear Fault
remains the existing visible recovery for a machine fault. Capture failures say
**CAPTURE FAILED — KEEP THE SETUP IN PLACE** and permit no programmed return.
The existing abort handler clears program probe selection. There is no new
recovery gate or state that requires a hidden reset.

Build through `scripts/build_native.sh`. The previously added M190 is discovered
on the next standard LinuxCNC task launch; this work does not restart the current
session or load/run the script. Offline arithmetic, persistence and export checks
are assistant-arranged checks, not proof of machine behavior.

The standalone interpreter checks exercised a stepped fixture with missed cells,
unset settings, an exceeded reach budget, a missing reference, failed fine contact,
failed release and obstruction capture. The compiled export checks exercised
fine-only heights, empty heights for misses, provisional output without a result,
and rejection of inconsistent exact bits and counts. The three existing Rust
capture tests returned passing results. No physical surface map has been taken.
