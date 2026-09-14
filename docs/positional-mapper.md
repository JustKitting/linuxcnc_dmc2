# Positional mapper commands and practice runbook

The standard `native/bin/dmc2ctl object-map` command now reads STL geometry and
fits retained probe contacts to it. It also measures explicitly assigned stock
faces and transforms named design points using a candidate placement. All these
commands operate on files before opening any machine connection.

The research, physical requirements and continuation design are in
[Probe-based positioning and manufacturing continuation](probe-based-continuation.md).
The earlier [object store guide](object-mapper.md) describes immutable objects,
setups and captured ledgers.

## Run the included numerical example

From the project root, choose a directory that does not exist:

```sh
bash examples/positional-mapper/run-example.sh /tmp/my-positional-example
```

The script uses the standard installed binary. It creates a separate store,
attaches an STL, imports synthetic trigger records, performs a fit, maps named
locations and exports its files. It does not open LinuxCNC, load a machine
program, move an axis, change an offset, clear a fault or restart anything.

The example's retained operands are:

| Item | Synthetic value and reason |
|---|---|
| Intended model | 40 × 30 × 20 mm cuboid; simple independent size reference |
| Actual reference placement | Translation (70, 60, 40) mm, yaw 7°; nonidentity transform checks frame direction |
| Initial guess | Translation (70.3, 59.8, 40.2) mm, yaw 6°; deliberately approximate |
| Ball radius | 1 mm; explicit synthetic spherical contact geometry |
| Trigger-to-ball vector | (0, 0, −20) mm; checks that mounting is actually included |
| Pretravel | 0.01 mm along each approach; checks that correction is subtracted |
| Reference contacts | Nominal X-min, Y-min and Z-max faces at separated locations |
| Independent checks | Separate records excluded from the fit |
| Extra stock | X-max at 42 mm and Y-max at 33 mm; stock does not influence placement |
| Bottom | Unmeasured; thickness must remain null |

The fixture is not a closed physical rim run. Its ledger and resulting analysis
remain marked synthetic/partial. Expected calculations are placement near the
retained transform, stock X/Y spans near 42/33 mm, unknown stock Z span, and the
separate ideal finished-model span 40/30/20 mm. Near-zero synthetic residuals
measure numerical arithmetic only; they are not probe accuracy.

The example's computational settings are explicit in `fit-request.txt`:
50 iterations, 0.000001 mm numerical step tolerance, 0.1 mm Huber transition,
2 mm association limit, 2 mm translation correction and 3° rotation correction.
These allow correction of the specified synthetic initial error. They are not
default settings for a physical part or statements of achievable accuracy.

## Preserve a real part and its measurements

The following IDs are examples, and paths identify files that must actually be
provided. Creating an object or setup records an identity; it does not assert
that a physical object was moved.

```sh
native/bin/dmc2ctl object-map create part 'Part for positional mapping'
native/bin/dmc2ctl object-map add-setup part first 'First retained setup'
native/bin/dmc2ctl object-map attach-design part native /path/to/part.FCStd
native/bin/dmc2ctl object-map attach-design part reference /path/to/part.stl
native/bin/dmc2ctl object-map import-capture part first rim /path/to/mapper-ledger.txt
native/bin/dmc2ctl object-map show part
```

The accepted capture workflows are circle, surface, gauge block and mapper.
Each import keeps the exact original payload. Fine contact references use its
original sequence numbers. Misses, coarse contacts, release events and stopped
positions cannot substitute for a selected fine trigger. Obstructed captures
are quarantined; partial captures remain explicitly partial.

`object-map --store /some/directory ...` selects a portable independent store.
The default is `var/objects` beneath the project. Snapshot conflicts require a
new ID. Reusing an existing different source never overwrites the retained one.

## Inspect scale and prepare a request

```sh
native/bin/dmc2ctl object-map inspect-stl /path/to/part.stl 1
native/bin/dmc2ctl object-map prepare-fit part first reference > /tmp/part-fit-request.txt
```

`1` in the inspection example means that the STL was intentionally exported in
millimetres. Use the known export-unit conversion for the actual file. The parser
reads ASCII and exact-length binary STL, including a binary header beginning
with `solid`. Inspection reports spans and edge/winding defects. Fitting rejects
open or inconsistently wound geometry and nonpositive signed volume. It does
not repair geometry, weld approximate vertices, or check self-intersections.

`prepare-fit` reads the attached STL revision and available fine contact IDs.
All settings initially say `REQUIRED`, and every contact initially has the
neutral `observe` role. This prevents an invented initial placement or
calibration from being used accidentally. Edit this data request, not the
production code or probing program.

| Field | Meaning and source |
|---|---|
| `design` | Attached STL revision ID |
| `model_role` | `finished-design` or `reference-stock`; identifies what the mesh represents |
| `stl_mm_per_unit` | Known export-unit conversion |
| `calibration_state` | `nominal`, `calibrated` or `synthetic`; a declared evidence category, not software certification |
| `calibration_reference` | Evidence/convention for mounting, radius and pretravel; identify any effect already included in effective dimensions |
| `frame_reference` | Shared machine reference and unchanged probe/object mounting for the selected captures |
| `ball_radius_mm` | Explicit radius under that calibration convention |
| `trigger_to_ball_mm` | Comma-separated machine-frame vector from retained trigger reference to geometric ball centre |
| `pretravel_mm` | Nonnegative advance along the recorded approach before trigger; subtracted once |
| `initial_translation_mm` | Initial model origin in machine millimetres |
| `initial_rotation_xyz_deg` | Initial Euler angles; composition is Rz × Ry × Rx, applied to model coordinates |
| `solve` | `translation-yaw` or `rigid` |
| `max_iterations` | Explicit computational budget |
| `convergence_mm` | Maximum fitted-contact displacement for a full numerical update to count as converged |
| `huber_mm` | Residual magnitude where robust weighting starts; chosen for the analysis |
| `correspondence_limit_mm` | Maximum absolute sphere/surface residual for every fitting row |
| `max_translation_correction_mm` | Maximum Euclidean shift of model origin from initial guess |
| `max_rotation_correction_deg` | Maximum relative rotation from initial guess |

Zero mounting or pretravel values are permitted **when explicitly declared**;
zero is not an inferred calibration. The single mounting/pretravel model in a
request must apply to all its selected captures. If the probe or machine
reference changed, separate the analysis until that relationship is established.

The CSV after the blank line selects observations:

```text
capture,sequence,use
rim,3,observe
```

Replace roles and add captures as appropriate. Valid roles are:

- `fit`: stable geometry represented by the registration mesh.
- `check`: independent observations withheld from fitting.
- `observe`: residual reporting only.
- `x-min`, `x-max`, `y-min`, `y-max`, `z-min`, `z-max`: explicitly identified
  stock faces assumed parallel to the corresponding model axes.

A record may appear once only. A single observation cannot both fit a placement
and independently check it. For unfinished stock, use known datum surfaces for
fitting and the stock-face roles for allowance measurements. If the whole object
is raw stock, supply a suitable reference-stock model or first establish its
reference features; do not fit all oversized faces to a finished-part mesh.

## Fit, inspect and repeat without editing control code

```sh
native/bin/dmc2ctl object-map fit part first candidate-a /tmp/part-fit-request.txt
native/bin/dmc2ctl object-map export-fit part first candidate-a /tmp/part-fit-a
```

Use a new analysis ID for a new parameter choice. The resulting bundle includes:

| File | Meaning |
|---|---|
| `manifest.json` | Final publication marker, identities, outcome, matrix, residual statistics and dimensions |
| `request.txt` | Exact settings and selected contact references |
| `source.stl` | Exact retained STL bytes, in its original units |
| `capture-<id>.txt` | Exact original ledgers used by the request |
| `residuals.csv` | Every selected trigger, role, corrected centre, nearest triangle, residual, weight, facing and association status |
| `model-candidate.machine-mm.stl` | Nominal geometry transformed by the proposed placement, already in machine mm |
| `ball-centres.machine-mm.asc` | Corrected centre points under the declared calibration |
| `estimated-surfaces.machine-mm.asc` | Surface estimates using the closest nominal normal, including observations that may not agree |
| `pose-candidate.txt` | Reusable proper rigid transform; published only for numerical convergence |

The estimated surface cloud contains hypotheses for visual inspection. Far
points, a wrong initial pose or a mismatching nominal surface can give an
incorrect normal. Read the matching residual row before interpreting a point as
an established physical surface. The stock-face calculation uses its declared
axis normal; the generic surface cloud uses the closest mesh normal.

Only `fit` rows drive the objective. Huber weights remain visible and no fitting
row is silently dropped. Reported independent-check residuals are not converted
into an acceptance verdict; the association limit is not a feature tolerance.
`cam_ready` remains false and uncertainty remains null.

Missing rank returns a readable error before geometry publication. An iteration
limit, search-bound stop or stalled fit retains an analysis report with its
reason but no reusable pose file. A directory lacking `manifest.json` is an
interrupted publication; preserve it and retry under a new analysis ID.

## Reuse the placement for named locations

Create a CSV of design-space features in millimetres:

```text
id,model_x_mm,model_y_mm,model_z_mm
model-origin,0,0,0
```

The origin here is mathematical model data, not an instruction to move there.
Then transform it:

```sh
native/bin/dmc2ctl object-map locate part first candidate-a /tmp/features.csv /tmp/feature-locations.csv
```

The output preserves the model coordinates, proposed machine coordinates,
object/setup/analysis identities, and the `unreviewed-coordinate-proposal`
state. This is the reusable input for later location and cutting scripts. It
contains no motion, feed, approach path or offset command.

Machine X direction labels remain physical RIGHT / LinuxCNC −X and physical
LEFT / LinuxCNC +X. A coordinate transform neither changes that mapping nor
chooses a direction of travel. An eventual motion program must still implement
its exact approved paths through the typed DMC2 script loader.

For a later placement, add a new setup, import its captures and fit another
candidate. The same `features.csv` can then produce locations for that setup.
The parent setup is never silently overwritten or assumed still valid.

## FreeCAD and the first physical experiment

Open `model-candidate.machine-mm.stl` in FreeCAD and import the `.asc` files
through the Points workbench. They are already in a common machine frame; do
not apply the candidate matrix a second time. Open the native design separately
when checking its feature history and CAM setup.

The next physical stage is a placement-prediction experiment using a matching
real model, reference contacts and independent check locations. Its movement
sequence will be prepared separately after the actual part and geometry are
known. The research document lists the inputs needed before generating later
cutting operations or a printer-specific continuation.

No new AXIS pane is added here. The current pendant scripts remain the
acquisition interface. These Rust file commands are the analysis interface and
can later be called by a dedicated UI without moving fitting work into the
motion-control path.

## Recorded TODOs — 2026-09-14

These items are pending. Recording them does not change the current acquisition
program or authorize a machine run.

- [ ] **Exponential edge bracketing, then binary refinement.** Replace the
  initial long traversal and repeated direction reversals with the user's
  growing search sequence: **1 mm, 2 mm, 4 mm, …**, bounded by the configured
  maximum and retained plate edge. Keep the last contact/no-contact observations
  that bracket the transition, then binary-search inside that bracket. Preserve
  the existing first direction: physical RIGHT / LinuxCNC -X. Define the step
  reference explicitly in the implementation so growing travel legs and
  distances from the initial position cannot be confused.
- [ ] **Growing local edge search with internal refinement.** Apply the same
  coarse-growth/local-refinement principle to edge detection and rim following
  instead of always advancing by fixed 1 mm increments. Retain the local contact
  and tracing plane, grow the coarse search until it brackets the next boundary,
  then refine within that interval. Continue following the outline from the last
  contact. Keep coarse search scale separate from final refinement resolution;
  preserve exact trigger capture, full probe-diameter backoff, plate bounds and
  the approved speed/depth policy. X labels remain physical RIGHT / LinuxCNC -X
  and physical LEFT / LinuxCNC +X.
- [ ] **Reserve a bounded fine re-touch allowance.** The run ending at
  16:56:48 EDT stopped at sample 77 with no fine trigger. Its original target
  allowed only 0.009887915 mm beyond the coarse trigger, while an earlier paired
  contact in the same run needed 0.020173821 mm. The full 2 mm backoff was
  recorded. `mapper-run.ngc` currently reuses the coarse target for the fine
  approach, so a coarse hit near that target can leave insufficient reach for
  observed coarse/fine variation. Specify the fine endpoint allowance and its
  bounds explicitly; retain the exact fine-capture requirement. Do not accept
  the missed endpoint or coarse trigger as a fine measurement. Source ledger:
  `tmp/output/mapper/mapper-1789419086126841028-1241227.txt`, records 405–409;
  paired comparison: records 359 and 363. No new allowance or retry behavior
  has been applied.
