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

Mapper imports also retain the original `.plate.txt`, `.feeds.txt` and
`.outline.txt` companions found beside the source ledger. The ledger and present
companions are published together in one `DMC2_OBJECT_CAPTURE_V2` record. Missing
files are explicit; empty or malformed files are retained for diagnosis. The
source files are not required after import. Existing V1 records still read, with
companion settings marked missing; they are never populated from current config
or from a source path that may have changed. To retain companions for a previous
V1 capture, import the original run under a new capture ID.

Show object displays the acquisition context and any settings-reader error.
Analysis and FreeCAD exchange exports include the exact companion bytes and a
`.context.json` description. Both acquisition and analysis use the shared
`probe_data/mapper_settings.rs` parser for recorded bounds, feeds, resolution and
policy versions. These are historical settings, not proof of current machine
state, probe calibration or motion authorization.

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
and independently check it. The STL registration path needs known corresponding
geometry. Use the stock-outline path below when raw stock has no such geometry.

## Estimate an irregular stock outline without a reference STL

The standard binary and the Object Mapper operation catalog now provide:

```sh
native/bin/dmc2ctl object-map prepare-stock part first rim > /tmp/stock-request.txt
native/bin/dmc2ctl object-map fit-stock part first stock-a /tmp/stock-request.txt
native/bin/dmc2ctl object-map show-fit part first stock-a
native/bin/dmc2ctl object-map export-fit part first stock-a /tmp/stock-a
```

`prepare-stock` reads the selected retained capture's original phase records.
Fine outline-enter/advance contacts become ordered `fit` rows; the independent
closing touch becomes `check`. Other fine observations remain `observe`. Failed
captures remain quarantined. Additional captures can be selected explicitly
when they share the declared reference. The request needs no nominal stock
dimensions, STL correspondence, rectangle or fixed number of sides.

Fill the generated `DMC2_STOCK_OUTLINE_REQUEST_V1` request through the existing
request editor. Its calibration fields have the same definitions as the
placement request above and use the same Rust correction path. Additional fields:

| Field | Meaning |
|---|---|
| `closure` | `open` or `closed`: declared traversal topology, not evidence of seam agreement |
| `surface_model` | `probe-centres`, or explicitly assumed `vertical-sides` |
| `neighborhood_span_mm` | Maximum contact-arc distance in each direction used for a local tangent estimate |
| `huber_mm` | Perpendicular residual at which local robust weights decrease |
| `max_iterations` | Computational budget for each local iterative fit |
| `convergence_mm` | Maximum local projection change for numerical convergence |
| `max_gap_mm` | Maximum adjacent contact spacing admitted to local neighborhoods and check associations |
| `max_z_span_mm` | Allowed height range for interpreting the observations as one XY slice |
| `max_fit_residual_mm` | Residual requiring local refinement or investigation; it never deletes a measurement |

The estimator performs iterative robust orthogonal regression along the ordered
ball-centre contour. Each station retains its source contact, neighbors, signed
residuals, weights and stopping reason. Shape is estimated locally. Coherent
deviations can indicate curvature or missing detail and are retained. `check`
and `observe` rows never drive the fit. Ball-radius correction follows the
estimated surface normal, not an oblique probe approach; the XY correction is
only emitted when `vertical-sides` was explicitly selected.

The analysis bundle contains `stock-outline.machine-mm.json`, all source
captures, the exact request, `residuals.csv`, the ball-centre cloud and
`refinement-requests.json`. Measurement requirements identify gaps, unresolved
local shape, failed numerical convergence, mixed heights, missing closure or
checks, and unmeasured wall slope. These are requirements for acquisition, not
machine commands. The tracer does not yet consume them automatically.

Interpolated segments remain modeling assumptions. This slice does not establish
a closed three-dimensional material volume, hidden concavities or wall slope.
Self-intersection analysis, multi-height/top reconstruction and placement of the
unchanged machining geometry inside measured material remain implementation
work. The report retains `solid_stock: null` and `cam_ready: false`.

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

The AXIS startup integration now registers an **Object Mapper** tab beside
Pendant and Custom Scripts. **Open object mapper** opens a nonmodal window;
its forms come from the installed Rust binary's `object-map catalog` response.
The same typed operation definitions validate CLI argument counts and provide
UI labels and field kinds. Existing pendant scripts remain the acquisition
interface. The mapper window owns file analysis, not machine state.

The normal UI sequence is List/Create object, Show object, Add setup, Attach
design, Import capture, Prepare fit request, Save request as, Calculate placement,
and Inspect analysis. Existing object/setup/design/analysis IDs appear in the
field selectors after their records have been read. Browse opens a nonmodal
path chooser with directory navigation and an exact-path entry. The form values
remain editable when correcting an input error.

Prepare fit request and Open fit request create editable drafts in the result
selector. Saving uses a new path and preserves conflicting existing content;
an identical save is idempotent. Earlier drafts remain in the selector when a
new result arrives. Inspect analysis presents separate report, residual-row and
retained-request views. Export analysis and Map named locations use the same
file contracts documented above. No numerical fit or calibration default is
invented by the UI.

The standard Rust child runs outside the Tk callback. Cancel analysis targets
only that owned offline child; it does not abort or clear the CNC. An interrupted
publication may retain partial files. Retry unfinished object/setup creation
with the same ID and label; use a new analysis ID or export path after inspecting
partial results. File errors clear on a subsequent successful operation. The
mapper and its path chooser expose the existing Clear Fault and Pendant Mode
actions without analysis-state prerequisites or modal grabs. Hide retains the
window's drafts/results and does not issue a machine command.

At this checkpoint the Rust binary is built and installed, and the AXIS entry
point includes the new pane. The existing CNC session has **not** been restarted
to load it. Disposable widget checks and the offline example reported no
remaining failures; these do not establish live CNC behavior or physical
acceptance. The code and file checks are retained in
`/home/kit/cnc-backups/mapper-ui-e7tu4teh`.

## Recorded TODOs — 2026-09-14

This is the continuing implementation list for the positional mapper and
manufacturing-continuation work. An item stays open until its stated outcome is
supported. Source implementation, installation, numerical results and physical
acceptance are separate milestones. Recording a task does not authorize a
machine run.

- [ ] **Exponential edge bracketing, then binary refinement.** Source and
  standard capture binary now use versioned policy V3 for new runs: **1 mm,
  2 mm, 4 mm, …** offsets from the initial top sample, in physical RIGHT /
  LinuxCNC -X, bounded by the retained plate/travel intersection. The last
  contact and first miss define the bracket; binary refinement stays inside
  it until the existing handoff distance is met. A plate-boundary contact
  remains an error with no inferred outside point. Old policy snapshots retain
  their original plate-first path. Numerical checks reported no failures for
  refinement, plate termination and historical replay. Physical behavior under
  this new policy has not been observed; that acceptance milestone remains open.
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
  paired comparison: records 359 and 363. A maximum 0.1 mm past the coarse
  contact has been proposed to the operator, using the existing outline
  resolution and recorded variation as context. The answer remains pending;
  no new allowance or retry behavior has been applied.
- [ ] **Represent failed capture cycles explicitly.** A fine search reaching
  its endpoint without contact is currently recorded as `travel` with fine
  stage, followed by an interpreter abort. Shared Rust failure types now classify
  missing coarse/fine triggers, unexpected transfer contacts and contact recovery.
  Object-map import retains the issue's record, reason and recovery instructions
  and quarantines the capture; acquisition replay uses the same classification.
  Both standard binaries have been rebuilt and installed. File replay of the
  retained failed run reports `fine-trigger-not-retained` at record 409, preserves
  the original ledger byte for byte and exports labelled diagnostic contacts with
  no ASC cloud. The readback is retained locally at
  `/home/kit/cnc-backups/mapper-finish-pt50q0l_/capture-diagnosis/readback.json`.
  This file result does not establish physical recovery or a corrected fine
  approach. Those outcomes remain open with the fine-allowance work above. Source:
  `object_map/capture.rs`, `probe_capture/mapper/state.rs`, and
  `live/nc_files/mapper-move.ngc`.
- [ ] **Reconstruct oversized, irregular measured stock.** The owner clarified
  that the wood will exceed the required cutting geometry and has no guaranteed
  exact shape. Minimize residuals to retained rim/top/side measurements while
  allowing dimensions, edge directions and surface shape to vary. Do not require
  a quadrilateral, parallel/perpendicular sides or corresponding nominal CAD
  planes. Local plane or rectangle summaries must retain the actual outline,
  coherent deviations, concavities and original contact references. The current
  named-face calculation assumes model-axis-parallel planes and does not provide
  this reconstruction. The standard `prepare-stock` / `fit-stock` path now
  estimates an unrestricted ordered XY contour with iterative Huber regression,
  explicit probe correction, original-contact references and withheld checks.
  Its source is in `object_map/positional/stock`; the shared probe correction is
  in `object_map/positional/probe.rs`. Twenty-two object-map numerical tests
  report passes, including curved/indented contours, oblique approaches,
  unsupported intervals and withheld contacts. These are computational results,
  not physical stock evidence. Represent observed, supported-empty and unknown regions;
  retain explicit uncertainty/modeling assumptions instead of filling unobserved
  volume by implication. Keep predicted operation stock separate from measured
  stock. This takes priority over matched-plane initial alignment for the raw
  wood workflow; see the owner clarification in
  [the research and implementation note](probe-based-continuation.md#owner-clarification-oversized-wood-without-an-exact-stock-model).
- [ ] **Close the acquisition/estimation loop.** Stock-outline analysis now
  emits source-referenced requirements for gaps, unresolved local shape,
  independent-check disagreement, mixed heights and unmeasured wall slope.
  Consume those requirements through the typed acquisition planner with retained
  bounds, exact trigger capture and operator recovery. Additional observation
  selection and tracing must serve stock reconstruction and containment; they
  are not separate end goals. No analysis requirement currently issues a probe
  command. Keep the pending fine endpoint allowance separate from this work.
  Capture import now retains the original acquisition companions and exports
  them with the ledger, so later planning can use that run's bounds and policy.
  The common settings parser is used by both standard binaries. A file-only
  import/export of the failed run remained readable after its copied source
  directory was moved; all exported original files matched byte for byte.
  The standard acquisition reader replayed that export and retained the original
  missing-fine-trigger diagnosis at record 409. Old V1 snapshots reported missing
  context instead of consulting live configuration. Readback artifacts are in
  `/home/kit/cnc-backups/mapper-context-gbmdc3ng`. Both binaries are built and
  installed. Twenty-four object-map and seven mapper numerical checks reported
  passes; these do not establish machine behavior. Adaptive next-contact planning,
  multi-height acquisition and material containment remain outstanding.
- [ ] **Optimize machining placement inside measured stock.** Fit the unchanged
  required geometry into the stock estimate using the allowed translations and
  rotations. Account for material shortage and unknown coverage separately;
  expected oversize must not pull stock surfaces onto finished-part surfaces.
  Report local deficits even if an aggregate penalty is small. Select any
  clearance/allowance objectives from actual setup context. For dice OP1, preserve
  `stage1_after`, including backing and envelopes; checking only finished blanks
  would omit required intermediate material. The current Huber STL registration
  objective and descriptive `model_role` do not implement this placement problem.
- [ ] **Validate placement and compare setups.** Retain named calibration
  evidence and reference-frame relationships, calculate independent feature
  prediction errors, and provide a reviewed placement state with an explicit
  feature tolerance. Add repeatability and uncertainty analysis grounded in
  repeated observations. Current local fit convergence alone does not accept a
  placement or establish uncertainty. Relocation must create a new setup and
  transform the same design-space feature coordinates through its new pose.
- [ ] **Improve registration and mesh handling where required.** Add usable
  initial alignment from genuinely corresponding measured or machined features,
  expose competing symmetric placements, and handle surface/mesh defects
  explicitly. Matched-plane alignment is optional; it must not become a
  prerequisite for unknown-shaped raw stock or replace its reconstruction and
  containment objective. Nearest-triangle
  lookup now uses an immutable, balanced bounding-box hierarchy in Rust,
  retaining original triangle IDs and the lowest original ID for equal-distance
  matches. Coordinate-gap bounds prune distant branches without a dimensional
  search tolerance. The hierarchy is built once per loaded mesh; the retained
  triangle vector cannot be mutated independently of it. Background on this
  class of point/primitive queries: [CGAL AABB tree manual](https://doc.cgal.org/latest/AABB_tree/index.html).
  The direct component normalization also avoids a reciprocal-overflow defect
  exposed by a subnormal-distance query in the previous implementation.
  Sixteen object-map numerical tests report passes. A separate comparison with
  the saved previous source used 2,361 numerical queries derived from the
  supplied CAD planes across five delivered meshes. It reported identical
  nearest points, triangle IDs and signed distances; normal components differed
  by at most 1.1102230246251565e-16 after the normalization correction. Combined
  target query times were 0.849/0.810 seconds before and 0.040/0.042 seconds with
  the index, excluding mesh loading. These are computational observations from
  that comparison, not physical accuracy or universal performance claims.
  The matched standard Rust binary is installed; the offline example returned
  an unreviewed proposal and exported its files. Comparison inputs, outputs and
  the previous binary are retained at
  `/home/kit/cnc-backups/mapper-spatial-arbj4k3j`. Initial alignment, symmetry
  handling and self-intersection analysis remain outstanding. Mesh closure
  checks alone do not establish absence of self-intersections.
- [ ] **Connect mapper results to the operator workflow.** Provide normal UI
  access to object/setup selection, retained captures, model revisions, analysis
  parameters, residual inspection and named-location export. Analysis must run
  outside machine control and remain cancellable without gating Clear Fault or
  Pendant Mode. The AXIS pane, catalog-driven forms, nonmodal file chooser,
  retained request editor, analysis readback and cancellation path are now in
  source, with the matched Rust command binary installed. The offline example,
  conflicting-save check and disposable widget checks reported no remaining
  failures. Activation in the running CNC session and operator observation
  remain outstanding; no CNC restart or machine operation was issued.
- [ ] **Implement the FreeCAD setup adapter.** Create a new manufacturing
  revision using the chosen model placement, measured/predicted stock and
  retained fixture geometry; regenerate the selected operations and export the
  result for review. Avoid applying a transform both to the model and through a
  work offset. Target the actual FreeCAD release and document structure. Current
  native part inputs now include the delivered `THREE_AXIS_GLUE_A` FCStd, STEP
  and STL files, plus a hash-bound descriptor of frames, body roles and native
  planar references. The CAD peer reports FreeCAD 1.1.1 with the LinuxCNC post;
  its required `ocl`/`opencamlib` dependency for 3D Surface is missing. The peer
  owns geometry and workstation CAM preparation. A native CAM Job document,
  actual tooling/stock/fixture inputs and accepted physical setup placement
  remain outstanding. The present STL/ASC exchange is available; automatic Job
  updates are not implemented. OP1 must preserve `stage1_after`, including its
  envelopes and backing; `stage1_targets` is finished-blank reference geometry.
  The descriptor's OP1-to-OP2 flip is already applied to stage2 geometry and
  must not be applied again during import.
- [ ] **Generate and check continuation/location programs.** Use the retained
  setup, current stock, desired geometry and tool/holder/fixture data to prepare
  the next operation, with an explicit collision/clearance model and the normal
  typed script-loading/recovery contract. Named coordinate CSV currently contains
  no toolpath, tool selection, feed, offset command or operation history. Those
  facts must come from the actual job before a cutting program is issued.
- [ ] **Implement the additive continuation adapter.** Combine measured
  geometry with the original slicer project/G-code and retained printer process
  state. Distinguish a planar layer restart from nonplanar repair and incomplete
  layers. Current inputs missing: printer/controller details and the original
  job. No printer continuation program has been generated or executed.
- [ ] **Run the practical acceptance sequence.** Obtain a matching real model
  and units, establish the probe/reference convention, predict withheld features,
  retain their actual contact errors, repeat after relocation, then evaluate a
  reviewed subsequent operation. Exact machine sequences and fixtures must be
  known before execution. Numerical examples and builds do not satisfy this
  physical milestone. Keep failed or unmeasured stages explicitly open.
- [ ] **Receive and prepare the dice wood-cutting job.** Preserve the CAD-side
  `THREE_AXIS_GLUE_A` bundle with file hashes, units, original targets, OP1/OP2
  geometry, intermediate stock, carrier assembly, transforms and stable feature
  IDs. The durable handoff directory is
  `/home/kit/cnc-jobs/cnc-polyhedral-dice`; revision folders belong under
  `incoming`, and directional message folders under `coordination`. Establish
  actual stock/material, tool and holder geometry, workholding and the
  CAD-to-machine relationship before producing a setup-specific machining
  proposal. The supplied dimensions, bond layer and reference cutter are CAD
  data, not measurements of the current setup. The correspondence authorizes
  file collaboration only, with no motion, probing, homing, energization,
  restart or machine-control changes.
  All 71 delivered source sizes and hashes match the CNC-side readback. Message
  003's descriptor hash also matches and binds 41 bodies and 158 planar
  references. These are model-only features with unestablished probe access.
  The standard Rust object mapper now retains 15 native/STEP/STL snapshots for
  the raw stock, preserved OP1 material, finished references and OP2 input.
  Their payloads match the delivered source bytes. Object ID
  `cnc-polyhedral-dice` has planned `op1` and `op2` records; each remains
  unregistered, without captures or placement candidates. The append-only
  workflow reply and import receipt are in the job's
  `coordination/cnc-to-cad/003-cnc-import-workflow.md` and
  `003-cnc-object-import-receipt.json`. Supplement 004 supplied both combined
  finished-blank STLs and 79 native planes per connected-stock body, including
  five backing datum planes per setup. Exact triangle ranges match all original
  source payloads. The combined meshes are imported under distinct OP1/OP2
  revisions; the native plane metadata remains CAD reference data, not captured
  contacts. `004-cnc-supplement-readback.json` retains the supplement hashes,
  triangle-range comparison, Rust STL inspection and object-store readback;
  `004-cnc-handoff-reply.md` acknowledges the transfer and accepted workflow.
  Incoming source snapshots and local job records are kept outside code
  publication; these file readbacks do not establish a physical setup or a
  machining result.
