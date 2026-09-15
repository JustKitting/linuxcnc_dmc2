# Positional mapper commands and practice runbook

The standard `native/bin/dmc2ctl object-map` command estimates irregular stock
outlines and local 3D surfaces from retained probe contacts. This raw-stock path
does not require the wood to match an STL or rectangle. A separate registration
path fits genuinely corresponding model features and transforms named design
points using a candidate placement. All these commands operate on files before
opening any machine connection. Horizontal footprint placement now searches
inside the estimated outline. Material checking connects that candidate to
supported 3D surface patches and produces geometry-specific measurement needs.
Stock-volume reconstruction and full 3D placement remain on the TODO list below.

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
Self-intersection analysis, assembly into a supported volume and placement of
the unchanged machining geometry inside measured material remain implementation
work. The report retains `solid_stock: null` and `cam_ready: false`. The surface
operation below fits local geometry from top and multi-height observations.

## Estimate local 3D stock surfaces

The normal Object Mapper catalog exposes **Prepare 3D stock surfaces** and
**Estimate 3D stock surfaces**, followed by the shared Inspect/Export analysis
operations. CLI equivalents are:

```sh
native/bin/dmc2ctl object-map prepare-stock-surface part first > /tmp/surface-request.txt
native/bin/dmc2ctl object-map fit-stock-surface part first surfaces-a /tmp/surface-request.txt
native/bin/dmc2ctl object-map show-fit part first surfaces-a
native/bin/dmc2ctl object-map export-fit part first surfaces-a /tmp/surfaces-a
```

Preparation includes original fine contacts across the selected setup's
nonquarantined captures. Retained mapper seam contacts and Automatic top map's
fine cell-centre contacts become `check`; the other
fine contacts initially become `fit`. Edit those assignments and remove any
unrelated captures from the request. The shared frame and probe calibration
must apply to every selected row. Withhold independent contacts as `check`;
`observe` reports residuals without changing the fitted surface. All selected
original rows and source ledgers remain retained.

The new `DMC2_STOCK_SURFACE_REQUEST_V2` uses the same calibration fields and
trigger-to-ball correction as the other analyses. Additional required settings:

| Field | Meaning |
|---|---|
| `neighborhood_mm` | Maximum 3D ball-centre distance from each fitting station for local surface estimation |
| `max_approach_angle_deg` | Largest difference between recorded approach directions admitted to one neighborhood |
| `huber_mm` | Perpendicular residual at which local robust weights decrease |
| `max_iterations` | Computational budget for iterative fitting and each covariance solve |
| `convergence_mm` | Maximum change in projected neighborhood points for numerical convergence |
| `max_support_gap_mm` | Largest projected distance to a neighbor admitted to local check associations |
| `max_fit_residual_mm` | Maximum local residual for check eligibility; excess remains a recorded measurement requirement |
| `no_contact_model` | `eroded-probe-sweep`; explicitly accepts the conditional no-contact interpretation below |
| `no_contact_allowance_mm` | Additional nonnegative bound on undetected contact/path-position error; together with declared pretravel it must be smaller than the ball radius |

Historical V1 requests retain their original contact-only interpretation and
report bytes. The normal request editor accepts both versions. Preparing a new
request produces V2 and leaves its physical allowance unset.

The local least-squares plane uses covariance eigenvectors. The need to choose
neighborhood scale and resolve normal orientation is described in the
[PCL normal-estimation tutorial](https://pointclouds.org/documentation/tutorials/normal_estimation.html).
DMC2's Rust implementation iterates Huber weights, retains all residuals, and
uses each recorded probe approach to orient the normal. Collinear or ambiguous
neighborhoods remain unresolved; a rim line does not acquire an assumed wall
slope. A neighborhood whose approaches conflict with its fitted normal remains
unresolved rather than supplying an arbitrary sign.

The retained calculation is:

```text
ball centre = original trigger + trigger_to_ball - pretravel * recorded unit approach
surface estimate = fitted ball centre - ball_radius * fitted outward normal
```

The local fit estimates the probe-centre offset surface. Ball correction follows
that estimated normal. It does not reconstruct concavities inaccessible to the
ball, prove a plane between observations, or establish unseen interior material.
Large neighborhoods can blend adjacent surfaces; coherent residuals and the
original observations remain available for refinement.

`stock-surface.machine-mm.json` retains a patch per fitting station, its source
neighbors, fitted centre, normal, corrected surface position, covariance
eigenvalues, weights and stopping reason. Independent checks do not affect the
fit. A check can associate only with a converged patch meeting the requested
local residual, inside its projected neighbor hull and within the requested
distance of a neighbor. These bounds describe **local planar interpolation**;
they are not measured material coverage. Unsupported or disagreeing checks
produce explicit requirements in `refinement-requests.json`.

The shared bundle also contains every selected trigger/centre/approach/feed in
`residuals.csv`, a ball-centre ASC cloud, the exact request and the original
capture/companion files. Inspect and Export analysis use that same bundle. The
report remains `unreviewed-stock-surface`, with `solid_stock: null`, unknown
unmeasured volume and `cam_ready: false`.

The installed command binary's file exercise retained 26 synthetic fine records:
25 fit rows and one withheld check displaced by 2 mm normal to an analytical
tilted plane. All trigger values and source bytes survived import/fit/export.
The check reported 2 mm disagreement; the maximum normal-vector discrepancy
was approximately 2e-15, a floating-point result, not physical accuracy. Readback
is in `/home/kit/cnc-backups/mapper-surfaces-amm3v64y/round-trip-readback.json`.
The command entry point also now preserves an existing terminating newline:
previously it added an empty selection row to prepared/reopened request text.
The corrected surface request, existing outline draft and positional request
all reopened byte for byte. Numerical checks reported 29 object-map passes;
none of these results establishes physical stock, control recovery or machining.

### Retained no-contact support

V2 consumes every original coarse `miss` in captures referenced by selected
fine contacts. Each miss must belong to a complete capture/return cycle, with
its original acquisition companions, matching reported endpoint and coarse
feed. Interrupted, quarantined or unsupported capture contexts produce a
readable error before publishing the analysis. Fine misses, coarse contacts,
and stopped positions cannot become substitute fine measurements.

The model uses the finite reported segment in machine millimetres:

```text
ball-centre path = reported machine path + trigger_to_ball
exclusion radius = ball_radius - pretravel - no_contact_allowance
```

There is no trigger on a miss. Its endpoints remain labelled reported travel;
they do not receive a fabricated trigger correction. The erosion assumes the
declared pretravel plus allowance bounds undetected contact and path-position
error. No physical allowance is supplied by default. The swept ball constrains
surface interpolation under that assumption; it does not certify empty stock
volume or fill unmeasured regions.

The shared support used by independent checks, material assessment and mesh
reconstruction excludes intersections with these finite sweeps. Whole-facet
intersection is checked: clear vertices cannot justify a triangle crossing a
detected gap. Contact-driven plane coefficients and every original row remain
retained. Conflicting contact/miss pairs carry both source identities, the
patch source, its normal and the corrected contact position. A contradictory
patch cannot supply checked material or reconstructed facets. Observation
selection retains these conflicts as pending requirements; a proposed repeat
of a positive contact alone does not resolve the contradictory miss/model.
Nonfinite distance arithmetic propagates a source-named error with recovery.

The standard binary is built and installed. Sixty-eight library numerical
checks reported passes. Its synthetic file workflow retained twelve original
fine triggers and one separate no-contact path through fitting and export.
The gap reconstruction retained 108 facets and 25 excluded lattice vertices;
the contact-only reference retained 128 facets. A conflicting model retained
paired disagreements and produced no supported facets. Material comparison
and observation selection retained the conflicts, with nine pending patch
requirements and no selected repeat claimed to resolve them. An interrupted
miss cycle was rejected without publishing an analysis. V1 report, CSV, ASC,
request and refinement bytes reproduced unchanged. Readback:
`/home/kit/cnc-backups/mapper-miss-support-w6q8lyxw/round-trip-readback.json`.
These are source, build and numerical/file milestones. Physical no-contact
error bounds, coverage, acquisition and recovery remain unobserved for this
change; no machine action or restart was issued.

## Place a machining footprint inside the estimated outline

**Prepare machining footprint** and **Place machining footprint** connect the
retained outline to an unchanged required-operation STL. They search translation
and yaw within explicit bounds. The stock boundary can be concave and have any
number of edges; no rectangle, convex hull, matched nominal planes or stock-to-
design registration objective is substituted.

```sh
native/bin/dmc2ctl object-map prepare-footprint part first stock-a required > /tmp/footprint-request.txt
native/bin/dmc2ctl object-map fit-footprint part first footprint-a /tmp/footprint-request.txt
native/bin/dmc2ctl object-map show-fit part first footprint-a
native/bin/dmc2ctl object-map export-fit part first footprint-a /tmp/footprint-a
```

Select the complete material that the operation must preserve. For dice OP1 this
is `stage1_after`, including backing and envelopes; finished blanks alone omit
required material. The source STL is retained exactly. A candidate applies only
a proper rigid transform after the declared unit conversion.

The `DMC2_FOOTPRINT_REQUEST_V1` contains:

| Field | Meaning |
|---|---|
| `outline_analysis` | Retained stock-outline analysis in this same setup |
| `design` | Attached STL revision containing required operation material |
| `required_geometry_role` | Explicit `operation-retained-material` declaration |
| `stl_mm_per_unit` | Known conversion from the source file's units |
| `model_origin_z_mm` | Fixed candidate model-origin Z; horizontal fitting does not establish its physical correctness |
| `model_origin_x_bounds_mm`, `model_origin_y_bounds_mm` | Allowed model-origin placement ranges as `lower,upper`; equal endpoints keep that coordinate fixed |
| `yaw_bounds_deg` | Allowed yaw interval, spanning at most a full turn |
| `required_clearance_mm` | Nonnegative horizontal clearance requested from the estimated polygon |
| `cover_radius_mm` | Maximum radius covering a subdivided projected triangle; smaller values reduce conservative coverage error |
| `max_cover_samples` | Computational budget to cover every original triangle |
| `placement_resolution_mm` | Requested gap between the best objective value and the remaining search bound |
| `max_evaluations` | Computational budget for placement evaluations |

Preparation fills only the selected analysis/design identities. Other values
remain `REQUIRED`; they are analysis data, not new motion limits, offsets or
automatic machine actions. Save and reopen the request through the normal
editor. A failed calculation leaves the request editable and existing outputs
preserved; use a new result ID for another published calculation.

The source must be a closed outline with explicit `vertical-sides` probe
correction. That is a retained interpretation of the XY slice, not evidence
that the entire stock has vertical walls. The operation checks local fit/gap/
height support and independent contacts. It compares the original capture bytes
and reproduced outline report to the source analysis. Changed data or a changed
estimator requires a new source analysis. Intersecting, coincident or reversing
outline segments remain readable errors; their shape is never replaced with a
convex approximation.

Every source triangle is projected onto XY and subdivided along its longest
projected edge until a centroid-centred disk of the requested maximum radius
covers that subtriangle. Original triangle IDs remain attached. A triangle whose
vertices lie inside a concave outline can still cross a missing corner; coverage
therefore includes its interior, not just vertices. An insufficient sample
budget reports an error instead of dropping triangles or silently coarsening.

For each cover sample, let `d` be the polygon's signed outside distance at the
candidate centre, and `r` its covering radius. Its local clearance lies between
`-d-r` and `-d` under the polygon model. The objective is the minimum of the
lower bounds over all covered triangles. Excess stock produces clearance;
it is not a mismatch to the required design. All local clearance deficits remain
in the report, even when an aggregate summary would appear small.

The bounded search evaluates cell centres and subdivides the cell with the
largest possible improvement. Its bound uses the translation half-diagonal
plus the maximum yaw chord displacement of the cover centres. It stops when
the requested clearance is reached, the remaining objective gap meets the
requested resolution, the computational budget ends, or floating-point cells
can no longer subdivide. The latter outcomes retain their best candidate and
remaining bound; they do not assert that all possible physical placements fail.
Bounds describe numerical calculations for this polygon and cover, not physical
accuracy or a globally closed material volume.

The bundle retains the request, source STL, every source-outline bundle file
under `stock-source-`, the transformed candidate STL, `search-history.csv` and
`residuals.csv`. Each residual row names its source triangle, model/machine
centre, cover radius, signed distance, local clearance/deficit bounds and nearest
source-outline segment. Inspect/Export analysis use the shared UI path. A
`pose-candidate.txt` is published only when the requested horizontal clearance
is reached; the existing named-location operation can transform design points
through this unreviewed candidate. The pose does not establish Z registration.

**Height, bottom support, taper, cavities and fixture/tool clearance remain
unresolved.** This is horizontal placement inside the estimated outline. A
successful horizontal calculation must not be used as evidence of full stock
containment. The manifest retains `three_dimensional_containment: unresolved`,
unknown unmeasured volume and `cam_ready: false`. The 3D surface and acquisition
work must supply the remaining material constraints before a cutting job.

The installed binary's synthetic file exercise imported a three-lobed contour,
retained all 97 fine contacts and fitted 96 boundary stations plus a withheld
check. Its initial neighborhood smoothed a peak past the residual bound; the
footprint operation rejected that source. A new outline using immediate
neighbors retained the same contacts and residual bound, with check disagreement
0.042693 mm. The placement search then improved its conservative clearance from
-2.199750 mm to +0.597026 mm for an unchanged synthetic 6 × 2 × 2 mm solid.
The export retained all 12 source triangle identities, 1,228 cover samples and
every original source file. The matrix reproduced exported vertices exactly in
the file readback. Thirty-five object-map numerical checks reported passes,
including a triangle crossing a concavity despite having all vertices inside,
yaw/translation searches and exhausted-budget retention. These are numerical
and file results only. Artifacts:
`/home/kit/cnc-backups/mapper-footprint-6q6bno42/round-trip-readback.json`.

## Check candidate material against the measured surfaces

**Prepare material check** and **Check candidate material** connect a retained
machining-footprint candidate to the retained 3D surface estimate in the same
setup. This identifies local material/clearance shortages and observation gaps
for the required operation geometry. It does not fit raw wood onto the design,
change the design's dimensions, or promote local inwardness to a closed solid.

```sh
native/bin/dmc2ctl object-map prepare-material-check part first footprint-a > /tmp/material-request.txt
native/bin/dmc2ctl object-map check-material part first material-a /tmp/material-request.txt
native/bin/dmc2ctl object-map show-fit part first material-a
native/bin/dmc2ctl object-map export-fit part first material-a /tmp/material-a
```

The `DMC2_MATERIAL_CHECK_REQUEST_V1` fields are:

| Field | Meaning |
|---|---|
| `candidate_analysis` | Footprint candidate retaining complete operation material, original STL and rigid pose |
| `surface_analysis` | Retained local 3D stock-surface analysis |
| `required_clearance_mm` | Nonnegative clearance requested from each eligible local surface |
| `surface_allowance_mm` | Explicit nonnegative bound used in the local surface comparison; it is not inferred calibration accuracy |
| `normal_band_mm` | Positive distance from the estimated plane over which the local interpretation is allowed |
| `cover_radius_mm` | Maximum **3D** radius covering each required subtriangle |
| `max_cover_samples` | Computational budget retaining coverage of every original triangle |
| `max_patch_comparisons` | Computational budget covering all selected patches, checks and required-geometry regions |

The draft fills only the selected candidate identity. Supply other values from
the intended analysis and evidence. The source outline and surface requests
must declare the same `frame_reference`; differing references produce a readable
error before output publication, with no implicit alignment. Matching text is
a declared relationship, not proof of physical registration. The source STL is
transformed again and compared byte for byte with the retained candidate STL.
Original capture bytes and the reproduced surface report must match the source
analysis. Missing or changed sources require resolving the source revision.

The shared triangle-cover routine now distinguishes horizontal projection from
full 3D coverage. Material checking uses the latter, retaining source triangle
IDs even on vertical geometry. For each region, its entire projected covering
disk must fit inside a local patch's measured-neighbour hull and a neighbour's
support-gap disk. The complete cover must also lie within the explicit normal
band. An unsupported patch is not extended across unseen space. Eligible fits
must converge and retain local residuals within the source request's bound.

Independent checks remain separate from fitting. Each local comparison names
its supporting check contacts, and any relevant disagreeing check makes that
region a check-conflict state. A patch without a check stays labelled as such.
All eligible local comparisons remain visible, including conflicting or
unchecked comparisons alongside checked ones.

Let `p` be the covered subtriangle's candidate centroid, `s` a fitted surface
point, `n` its measured outward normal, `r` the cover radius and `a` the explicit
surface allowance. With `d = (p-s) dot n`, bounds on minimum local clearance are
`[-d-r-a, -d+a]`. The lower bound covers the whole subtriangle; its centroid lies
on the original geometry and supplies the upper bound. Requested-clearance
deficit bounds are retained for every comparison. No averaged penalty hides a
local shortage. These are conditional, floating-point model bounds, not a
measurement uncertainty certificate or proof of physical material.

The region states distinguish unsupported coverage, missing independent checks,
check disagreement, contact/no-contact model conflict, local shortage, an unresolved clearance interval, and local
inwardness. `measurement-needs.json` ties unresolved regions to their original
triangle, machine-frame region centre and covering radius. The required facet's
normal describes the design region; it is never substituted for a measured
stock normal. These data are not probe endpoints or approved approaches. The
remaining acquisition planner must use actual bounds and supported access before
turning them into an operator-reviewed probing operation.

The bundle retains the entire candidate under `candidate-source-` and the entire
surface analysis under `surface-source-`, including original triggers and
companions. Shared Inspect/Export analysis display the report and all residual
rows. Existing outputs remain immutable and another calculation uses a new ID.
`solid_stock` stays null, unmeasured volume stays unknown, and `cam_ready` remains
false. Top/side/base closure, cavities, fixtures and full material containment
are still outstanding, even when every local region lies inward of a plane.

Oriented local planes and undefined distance outside sampling support are
described in Hoppe et al., *Surface Reconstruction from Unorganized Points*,
section 3.4, pp. 3–4 of the [author's paper](https://hhoppe.com/recon.pdf).
Its projection formula has an [author-published correction](https://hhoppe.com/thesis.errata.html).
DMC2's bounded full-triangle comparisons, retained approach orientation,
independent checks and explicit unknown-volume state are the implementation
here; this does not implement the paper's complete reconstruction algorithm.

The installed binary's synthetic file exercise combined the retained lobed
outline with an unchanged 6 × 2 × 2 mm required solid at candidate Z=45 mm and a
partial 4 × 4 mm top grid at Z=45 mm. All 26 original fine contacts and 12 source
triangles survived export, with 1,920 full 3D regions: 534 local-clearance
shortages, 66 unresolved clearance bounds, 392 locally inward regions and 928
unsupported regions. Every region needing local resolution has a corresponding
measurement requirement. Retained files matched byte for byte and the request
reopened unchanged. The maximum local deficit interval was [1.15, 1.60] mm under
the synthetic request's allowances. A deliberately differing frame reference
was rejected before an output directory was published. Earlier surface and
footprint calculations reproduced their retained numerical/geometry files
byte for byte after the shared-code extraction. Forty-one object-map numerical
checks reported passes. These are numerical and file observations only.
Artifacts: `/home/kit/cnc-backups/mapper-material-prtkle5x/round-trip-readback.json`,
`compatibility-readback.json`, and `frame-mismatch.stderr` in the same directory.

## Reconstruct the measured stock surface

**Prepare stock reconstruction** and **Reconstruct stock surface** now turn a
retained 3D surface analysis into an indexed surface estimate and an STL. The
shape comes from the measured local patches. Calculation bounds crop the
numerical domain; they do not assert a rectangular stock shape or force measured
wood onto the required machining geometry.

```sh
native/bin/dmc2ctl object-map prepare-stock-mesh part first surfaces-a > /tmp/mesh-request.txt
native/bin/dmc2ctl object-map reconstruct-stock-mesh part first mesh-a /tmp/mesh-request.txt
native/bin/dmc2ctl object-map show-fit part first mesh-a
native/bin/dmc2ctl object-map export-fit part first mesh-a /tmp/mesh-a
```

`DMC2_STOCK_MESH_REQUEST_V1` uses these explicit fields. The prepared draft fills
only the selected source identity. Other values must come from the intended
calculation and retained support, rather than new machine defaults.

| Field | Meaning |
|---|---|
| `surface_analysis` | Retained 3D patch analysis in this object/setup |
| `bounds_min_mm`, `bounds_max_mm` | Finite calculation bounds in LinuxCNC machine millimetres, with positive span on each axis |
| `grid_spacing_mm` | Maximum lattice interval; actual axis intervals divide the requested bounds evenly |
| `normal_band_mm` | Distance from a checked local plane over which its field estimate is allowed |
| `max_interpolation_residual_mm` | Maximum facet-vertex disagreement with a common supporting measured patch |
| `max_grid_vertices` | Explicit lattice memory/computation budget |
| `max_field_comparisons` | Budget for local field lookups, independent checks and candidate-facet support comparisons |
| `max_mesh_triangles` | Candidate-facet budget, including facets later retained as unsupported |

Surface loading and local eligibility are shared with material comparison.
Original capture bytes and the reproduced source report must match the retained
analysis. Fits must converge within their source residual bound; independent
checks stay separate. The nearest original fitting station determines each
lattice value. If its fit, projected support, normal band or independent check
is unresolved, that vertex stays unresolved; a farther eligible plane does not
replace it.

A conforming tetrahedral subdivision extracts linear sign crossings, reusing
indexed intersections on common edges. Exact endpoint crossings share the
original lattice vertex. Facet winding follows the local field's outward
orientation. Each exported triangle also needs a common checked patch agreeing
with its orientation, containing its full projected area in the source hull and
a single retained neighbour's support-gap disk, and bounding the triangle's
plane disagreement. Missing-field tetrahedra and unsupported facets remain
explicit gaps. No hole filling or deletion of original contacts occurs.

The analysis retains the entire source bundle under `surface-source-`, together
with its request and these reconstruction files:

| File | Retained information |
|---|---|
| `stock-surface.machine-mm.stl` | Supported estimated facets; omitted when none are supported |
| `residuals.csv` | Every lattice point, signed distance or unresolved state, and nearest original contact |
| `mesh-vertices.csv` | Each generated vertex, original lattice endpoints and interpolation fraction |
| `mesh-facets.csv` | Every candidate facet, vertex IDs, cell/tetrahedron identity and supporting original contact or unresolved support |
| `unresolved-cells.csv` | Calculation regions containing missing field support |
| `measurement-needs.json` | Reason counts, recovery explanations and links to the source-linked region tables |

The manifest reports open boundary edges, nonmanifold edge incidence and winding
conflicts. Closed edge incidence alone does not establish vertex manifoldness,
absence of self-intersections, a solid or physical material. The STL is a
measured-surface estimate for inspection and downstream modeling. Unobserved
regions, sub-grid features and missing top/side/base connections remain explicit;
`solid_stock` is null and `cam_ready` is false. Measurement regions are data for
the acquisition planner, not motion endpoints or authorization to probe.

The installed-binary file exercise reused the retained synthetic top grid. Its
calculation box extended past the measured perimeter. The output contained 128
facets covering the expected 16 mm² at Z=45 mm, with 32 open perimeter edges,
440 unsupported lattice points and 320 unresolved cells. Every mesh vertex
reproduced its retained grid interpolation; all ten source files and eighteen
export files matched byte for byte. The previous material comparison reproduced
all 31 non-manifest files unchanged; its manifest differed only by analysis ID.
Forty-nine object-map numerical checks reported passes, including shared edges,
lattice zeros, outward closed sphere incidence, missing field support and
independent-check disagreement. These are numerical/file results, not physical
stock evidence. Readback:
`/home/kit/cnc-backups/mapper-stockmesh-uery5t54/round-trip-readback.json`.
The standard binary is built and installed; the CNC session was not restarted.

## Export a stock scene for the CAD workflow

**Export stock scene for CAD** combines a retained material assessment with a
stock reconstruction from the same surface revision:

```sh
native/bin/dmc2ctl object-map export-stock-scene part first material-a mesh-a /tmp/stock-scene-a
```

The standard UI supplies separate Material analysis and Stock mesh analysis
selectors. Export checks exact source file sets and bytes in both dependent
analyses, reproduces the stock reconstruction and checks its retained results,
and reproduces the candidate STL from its original geometry, unit conversion
and rigid transform. The candidate outline and measured surface must declare
the same reference frame. Differing revisions or bytes produce a named error
before publication, with instructions to select or calculate matching sources.

The new folder contains complete `material/` and `stock/` analysis bundles,
`README.txt` and a final `manifest.json` using
`dmc2.freecad-stock-scene.v1`. Its `geometry` list distinguishes unchanged
**operation-retained-material** from **estimated-measured-stock-surface**. Both
listed meshes are in machine millimetres and use identity import placement.
The required material has already been transformed; applying the candidate
matrix, a work offset or a setup flip again would duplicate that placement.
The original design STL, original unit conversion and model-mm-to-machine-mm
matrix remain separately identified for native CAD integration. An absent
supported stock mesh remains absent from the geometry list.

The scene retains material/stock reports and measurement needs. Export replays
the stock reconstruction; it preserves the existing material assessment with
exact source binding but does not rerun that assessment. This distinction is
explicit in the manifest. The files supply the common scene for inspection and
subsequent integration, not an accepted physical placement or native CAM Job.
Open stock regions, containment, actual tools and fixtures remain unresolved.
The CAD-side adapter must consume the intended operation geometry and apply the
accepted frame convention once when those inputs are established.

The installed standard binary exported the retained synthetic dataset with
both geometry roles. All 32 material-bundle files and 18 stock-bundle files
matched the originals. A deliberately altered disposable source copy was
rejected before any export directory was published, identifying the differing
capture filename. These are file observations; FreeCAD import, native CAM and
machine behavior were not exercised. Readback:
`/home/kit/cnc-backups/mapper-scene-58tfebd7/round-trip-readback.json`.

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

For the owner's oversized wood workflow, the next physical observations concern
the actual stock and independent checks. A matching raw-stock CAD model is not
a prerequisite. Nominal required machining geometry remains separate; dice OP1
must retain `stage1_after`, including backing and envelopes. Any movement
sequence requires its exact separate authorization. The research document lists
the inputs needed for later cutting operations or printer-specific continuation.

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

## Bounded 3D placement in reconstructed stock

The standard binary and typed Object Mapper catalog now connect a reconstructed
measured boundary to full rigid placement of unchanged required operation
geometry. This is an enclosed **stock model**, not an assertion about physical
wood. It does not require a rectangle, quadrilateral, convex outline or nominal
stock dimensions. For dice OP1 the required design remains `stage1_after`, with
backing and holding envelopes, rather than only the finished blanks.

```text
prepare-volume-placement OBJECT SETUP STOCK-MESH-ANALYSIS DESIGN
fit-volume-placement OBJECT SETUP NEW-ANALYSIS REQUEST
```

Use the existing request editor, Save request as, Inspect analysis and Export
analysis. The draft fills the selected source IDs; every other value is REQUIRED.
The source reconstruction is reproduced from its retained surface analysis and
exact original capture bytes before placement. Any absent mesh stays absent.

The request is `DMC2_VOLUME_PLACEMENT_REQUEST_V1`. Its explicit inputs are:

| Fields | Meaning |
| --- | --- |
| `stock_mesh_analysis`, `design` | Retained reconstruction and full required-operation STL revision. |
| `required_geometry_role` | `operation-retained-material`; required backing and holding features belong in this geometry. |
| `stock_occupancy_model` | `single-shell-enclosed-solid` explicitly models the inside of one closed boundary as material, without hidden cavities. It is not a measured physical fact. |
| `stl_mm_per_unit` | Original design unit conversion; reconstructed stock is already machine millimetres. |
| `model_origin_x_bounds_mm`, `model_origin_y_bounds_mm`, `model_origin_z_bounds_mm` | Allowed lower,upper translations of the unchanged design origin; equal endpoints keep a coordinate fixed. |
| `roll_bounds_deg`, `pitch_bounds_deg`, `yaw_bounds_deg` | Allowed rotations. The matrix convention is `Rz(yaw) Ry(pitch) Rx(roll)` followed by translation. Each interval is at most one full turn. |
| `required_clearance_mm`, `surface_allowance_mm` | Nonnegative clearance and an explicitly subtracted allowance. Neither supplies a physical uncertainty estimate. |
| `cover_radius_mm`, `max_cover_samples` | Full spatial triangle-cover size and computation budget; the source STL and triangle order are preserved. |
| `placement_resolution_mm`, `max_evaluations` | Stop when requested clearance is found, the remaining objective gap reaches resolution, the evaluation budget ends or a cell cannot be subdivided. |
| `max_topology_visits`, `max_winding_terms` | Explicit geometry-computation budgets. Winding reserves the requested search and final report across every cover sample and stock triangle; no partial geometry substitutes on exhaustion. |

Before enclosed-stock interpretation, the Rust geometry path checks consistent
outward closed edges, a connected cycle around every vertex, one connected shell,
and intersections beyond shared edges/vertices. Open, pinched, disconnected and
intersecting boundaries return readable source-specific recovery instructions.
The code does not cap holes, delete triangles or infer nested cavity semantics.
Adaptive orientation signs use the pinned
[GeoRust `robust` 1.2.0 predicates](https://docs.rs/robust/1.2.0/robust/).

The inward distance combines the nearest boundary distance with the sum of
oriented triangle solid angles. For a closed embedded outward boundary, winding
is one inside and zero outside; half separates these values. This is the
[closed-boundary winding interpretation described by libigl](https://libigl.github.io/tutorial/#generalized-winding-number),
not a machining tolerance or a way to fill open surfaces.

For each covered required triangle, the lower clearance is:

```text
inside_distance(transformed_cover_center) - cover_radius - surface_allowance
```

The search maximizes the minimum of these values. Translation half-diagonals and
rotation chords bound how far any covered point can move within each search
cell. Signed distance is 1-Lipschitz, so that displacement bounds the possible
objective improvement. These are floating-point calculations for the stated
geometric model, not a formal arithmetic or physical certificate. Each rotation
acts around the original model origin; long offsets can therefore make bounds
conservative. Smaller covers reduce geometric conservatism but increase work.
The implementation sums all stock-triangle angles per query; budgets and the
existing UI Cancel remain available for expensive analyses.

Every result retains the unchanged design STL, complete stock/source bundles,
transformed candidate STL, full residual rows, improving-search history and an
unreviewed pose record. The best candidate and remaining objective upper bound
survive budget/resolution termination. A retained pose is not accepted placement.
`nearest_stock_triangle` indexes the retained stock STL's original triangle order;
reconstruction facet/source tables remain alongside it. The existing material
checker and CAD scene exporter use the shared typed candidate reader for both
footprint and volume candidates. A volume scene must use the same exact stock
revision as its candidate. Scene meshes are already in machine millimetres and
import at identity; no additional flip or candidate transform is applied.

The local material report remains a local surface assessment: a deeply interior
point can lack local patch support even when the explicit enclosed model treats
it as material. Its measurement regions are not executable probe targets. Stock
occupancy, fixture/tool clearance and physical registration remain unaccepted.

The installed-binary numerical file exercise is retained at
`/home/kit/cnc-backups/mapper-volume-1_k2fict`. The synthetic ellipsoid uses
10/9/8 mm semiaxes and explicitly labelled synthetic calibration. Earlier sparse
analyses remain open. A denser set of 800 fitting and 400 withheld contacts, a
2.5 mm neighborhood and a 0.5 mm grid produced 35,356 supported facets with closed
edge incidence; the placement path then checked 17,680 vertex links and 262,348
intersecting-AABB triangle pairs. It moved the unresized 12-triangle required
geometry from a lower clearance of -2.1275388512570115 mm to
+1.8029034233368737 mm, using 512 covers and three objective evaluations. These
numbers describe this calculation only; none is a production default.

The 25-file placement export and 60-file CAD scene retained every source file
byte for byte. A deliberately budget-limited request retained its best pose;
an open reconstruction was rejected before publishing a placement directory.
The original footprint replay matched all 17 files except the new analysis ID,
and the older material/scene outputs matched their retained bytes. Source tests
reported 58 passes, including concavity, pitch search and mesh-intersection
cases. Readbacks: `round-trip-readback.json`, `legacy-readback.json` and
`legacy-final-readback.json` in that backup directory. These are source/build/
numerical/file milestones, not evidence of physical motion, material occupancy,
measurement accuracy, accepted workholding or a usable cutting program.

## Material-directed follow-up observations — 2026-09-15

The standard `dmc2ctl object-map` catalog now includes **Prepare follow-up
observations** (`prepare-observations`) and **Select follow-up observations**
(`plan-observations`). They use the existing request editor, inspection and
export operations. Select a retained material assessment, fill both budgets,
save a new request and select a new analysis ID:

```text
DMC2_OBSERVATION_REQUEST_V1
material_analysis=REQUIRED
max_observations=REQUIRED
max_candidate_need_comparisons=REQUIRED

```

The source material assessment is reproduced from its original candidate and
surface inputs before selection. Original ledgers and acquisition companions
must match the surface bundle byte for byte; current settings never substitute.
The analysis preserves all source files under `material-source-` and publishes
its manifest last.

Selection groups missing independent checks, disagreeing checks and checked
local shortages by their source surface patches. A greedy choice addresses
the largest number of still-unplanned patch requirements, with source-order
ties. Required mesh subdivision density does not increase a patch's weight.
Identical requests within a capture share a proposal while retaining every
original contact reference. The observation budget limits the chosen set;
remaining requirements and unavailable approaches stay in the report. This
selection is not a global optimum or an execution sequence.

Each candidate uses the shared acquisition `Request` and retained-cycle reader
through `probe_data/mapper_trace/observation`. A fine trigger and a ready endpoint
must both exist. The report retains their distinct coordinate frames, the
original phase, approach, target, rates, bounds and work-to-machine translation.
Original-clearance entry and a released outline approach are different typed
prerequisites. No transfer or descent to an outline contact is inferred from
the required model, measured normal or old endpoint. X labels remain physical
RIGHT / LinuxCNC -X and physical LEFT / LinuxCNC +X.

The resulting `observation-plan.machine-mm.json` is a **proposal**, with no
runtime sequence, entry path, execution order or machine authorization. Existing
contact roles are unchanged. A future repeat can become a new independent check
only after its own exact trigger capture and readback. All material requirements
remain unresolved pending that evidence; selected proposals do not clear them.
Repeating a contact cannot supply missing spatial support, wall slope or closed
volume coverage. Regions with no supported approach remain regions of interest.
The pending fine re-touch allowance remains a separate, unapplied proposal.

Installed-binary file exercise:
`/home/kit/cnc-backups/mapper-observations-2ie4uu5i/round-trip-readback.json`.
The retained synthetic partial-top example supplied 26 fine contacts and 25
patch requirements. Selection chose the original request for `partial-top:38`,
which can check the overlapping patches; all 1,528 unresolved material regions
remained unresolved. All 25 candidate requests matched their original approach,
target, phase and fine feed. The 32 source files and all 36 exported files
matched their retained bytes. Replaying the material calculation matched its
old files except the new analysis ID. The request editor round trip preserved
its bytes. A changed companion snapshot and an undersized computation budget
were rejected before publishing an analysis directory. Source numerical checks
reported 61 passes. The standard binary was built and installed; none of these
results establishes physical probing, current clearance or material occupancy.
The CNC session was not restarted. The running UI's adoption of the new catalog
entries remains unobserved.

## New spatial top observations — 2026-09-15

**Prepare new top samples** (`prepare-spatial-observations OBJECT SETUP
MATERIAL_ANALYSIS SOURCE_CAPTURE`) supplies a separate request type to the
existing **Select follow-up observations** operation. The request editor,
inspection and export use the normal installed `dmc2ctl` and Object Mapper
catalog. Historical repeat requests retain their original interpretation.

```text
DMC2_SPATIAL_OBSERVATION_REQUEST_V1
material_analysis=REQUIRED
source_capture=REQUIRED
sample_spacing_mm=REQUIRED
max_observations=REQUIRED
max_grid_cells=REQUIRED
max_candidate_comparisons=REQUIRED

```

The selected capture must contribute original fine contacts to the reproduced
material assessment. Its ledger and original acquisition companions, including
absent companions, must match the retained surface source. Only Surface and
Automatic top map captures with complete vertical top-search cycles qualify.
Probe radius must agree with the source surface model. Partial ledgers can
supply complete cycles; an interrupted cycle or quarantined capture cannot.

Unsupported required-geometry cover regions supply **XY areas to investigate**.
Their shape, normals and height do not supply a stock model or probing depth.
For a downward column, the conversion is:

```text
region_work_xy = candidate_region_machine_xy
                 - trigger_to_ball_xy
                 - original_work_to_machine_xy
```

Downward pretravel has no XY component. Candidate cell centres are aligned to
the original work-XY origin using the explicit sampling spacing, at least the
largest retained XY step. Cells intersecting each projected cover disk are
grouped once, so overlapping mesh triangles do not multiply a cell's selection
weight. The proposed point can be outside a particular disk by up to half the
cell diagonal. Candidate centres stay inside the original plate/travel
intersection. Censored projections and regions with no candidates remain
explicit in the report.

The shared typed `TopColumn`/`Request` retains original starting clearance,
descent floor, mounted-reach floor, downward/fine/travel feeds and backoff.
Sampling spacing does not alter the original fine backoff or descent budget.
Both original fine and coarse-miss columns are excluded from selection when
their requested XY matches within the retained endpoint-step comparison.
Remaining cells are prioritized by smallest XY distance to an original searched
column, then grid index, without triangle-count weighting. This priority is not
an execution order. The observation limit selects a subset; all cells and
region associations remain retained. Insufficient cell/comparison budgets
produce a readable error before publication instead of a truncated assessment.

`observation-plan.machine-mm.json` uses `dmc2.spatial-observation-plan.v1`.
It retains source identity, frame operands, actual acquisition-contract fields,
all unresolved material regions, and null new triggers/execution order. No
existing contact becomes a new check. New top sampling alone does not resolve
side/underside support, an unobserved surface normal, a contact/miss conflict or
closed material volume. Entry and execution still require a reviewed path and
fresh exact trigger capture/readback. X labels remain physical RIGHT /
LinuxCNC -X and physical LEFT / LinuxCNC +X.

The installed-binary synthetic file exercise produced ten new candidate cells
and selected four at 0.5 mm spacing. A request whose cell centres matched old
columns selected none, retaining fine sources 53/58 and coarse-miss source 61.
All 1,662 unsupported material regions remained unresolved; 1,632 projected
regions crossed the original envelope. The 32 material-source files and all
36 exported files matched their original bytes; the old repeat report and
editor round trip retained their bytes. Undersized comparison/cell budgets and
an unrelated source capture published no analysis directory. Readback:
`/home/kit/cnc-backups/mapper-spatial-r25wc__g/round-trip-readback.json`.
Seventy-one library numerical checks reported passes. The standard binary is
built and installed. These are file/numerical results, not physical acquisition
or machine-recovery evidence; the CNC session was not restarted or moved.

## Executable top follow-ups — 2026-09-15

Object Mapper's **Export top follow-up program** operation accepts a retained
new-top-sample analysis:

```text
export-top-observations OBJECT SETUP ANALYSIS NEW_DIRECTORY
```

The export reproduces the material assessment and observation selection and
compares their retained sources, report, residuals and manifest before writing.
An empty selection or a repeat/side request is a readable error; no points are
silently omitted. The output includes `execution-order.csv`, the full original
analysis, entry instructions, an inspectable `.followup.txt` and the typed
`top-followup.ngc`. The program is published last. Its explicit execution order
is the selected priority order, with the existing return to original clearance
between columns. No additional path optimization is applied.

Use **Reload operations** in an already-open Object Mapper to obtain the new
catalog. Review the source probe/stock reference, required starting coordinates,
work offset and every transfer at the original clearance plane. Export does not
load, position or run the machine. After that review, use standard AXIS File
Open and Run. The generated file declares motion, digital output, coordinate
state and external-command effects, the existing all-homed prerequisites and
Abort recovery. X increasing is physical LEFT / LinuxCNC +X; decreasing is
physical RIGHT / LinuxCNC -X.

The export materializes the shared `mapper-run.ngc` subroutine definition,
renaming its local subroutine identity and omitting only its standalone file
terminator. There is one executor implementation. This keeps M190 P12 in the
exported file: LinuxCNC 2.9.10's `emcTaskUpdate` obtains `task.file` from the
interpreter's **current** filename, which changes inside external subroutines
(`emctask.cc`, `rs274ngc_pre.cc::file_name`, `interp_o_word.cc`). The native
binding reads status only. Program regeneration compares the entire body,
including its encoded plan bytes and shared template. The normal loader also
binds the program revision at File Open/Run. No mutable current-plan flag exists.

Mapper mode `TopFollowup` retains the original plate/feed snapshots, start
fields and explicit work-XY columns. Before a target is published, the fresh
start fields must match the source using the existing decimal-serialization
comparison. This is not a mechanical alignment tolerance. The shared original
floor, feeds, double-touch, capture/readback and withdrawal remain in use.
Complete fresh cycles must agree with their planned rows, directions and feeds;
a final result must contain the requested number of cycles. Coarse misses have
no invented trigger or surface height.

Import the fresh ledger with its `.plate.txt`, `.feeds.txt` and `.followup.txt`
companions under a new capture ID in the original object/setup. Capture storage
V3 retains the follow-up bytes; historical V1/V2 encoding and context JSON stay
unchanged when that companion is absent. Missing plans, mismatched settings,
wrong rows and unfinished cycles receive `followup-context-invalid` quarantine
with the actual diagnosis. Their original bytes remain exportable and valid
captures remain available to normal preparation. Fresh top fine contacts are
`fit` rows; existing independent check roles remain unchanged. Calculate a new
stock surface and material assessment before selecting further observations.

Both standard binaries are built and installed. The installed synthetic file
workflow exported four columns, retained three fine contacts and one coarse
miss, preserved all 36 analysis source files and the fresh ledger/companions
through surface and FreeCAD export, and reached a new material assessment and
next spatial-request template. Missing-plan, changed-start and interrupted
captures retained separate quarantine diagnoses while preparation still selected
valid rows. Empty and wrong-request exports created no directory. Readback:
`/home/kit/cnc-backups/mapper-followup-ync_w2m4/round-trip-readback.json`.
These are file and numerical results, not physical acquisition or recovery
proof. The session was not restarted or moved. Read-only UI inspection reported
both recovery buttons `normal` and no modal grab.

## Recorded TODOs — 2026-09-14

This is the continuing implementation list for the positional mapper and
manufacturing-continuation work. An item stays open until its stated outcome is
supported. Source implementation, installation, numerical results and physical
acceptance are separate milestones. Recording a task does not authorize a
machine run.

- [ ] **Run material-directed follow-ups through the normal UI.** The top
  program export, shared executor, fresh-plan capture binding, import and
  surface/material continuation are implemented and installed; see the runbook
  above. Physical entry, acquisition and recovery observation remain open.
  Repeat/side plans still need explicit entry/recovery geometry and execution;
  no unsupported requests are silently narrowed to top points. Further spatial
  selection must pool compatible acquisition histories so choosing a recent
  capture does not reselect columns recorded in an earlier capture. Independent
  follow-up check acquisition, multiheight sides and unseen volume remain parts
  of the whole stock-to-CAM workflow.
- [ ] **Acquire automatic top coverage for irregular stock.** The production
  Rust planner and Scripts catalog now provide Automatic top map, sharing the
  existing mapper executor, original trigger capture, feeds and recovery. It
  grows cardinal searches from the entered grid spacing, refines measured
  contact/miss brackets, discovers the connected top grid and acquires fresh
  cell-centre checks. No rectangle fit is required. Plate-boundary contacts and
  out-of-envelope grid neighbours remain censored. Fine centre contacts become
  withheld `check` rows in normal 3D stock-surface preparation. Both standard
  binaries are built and installed. Twelve mapper and 61 library numerical
  checks reported passes. The installed file workflow retained 13 synthetic
  fine contacts (nine fit, four check), original triggers and source snapshots
  through fitting and both exports. An interrupted capture retained its
  diagnosis with no inferred coverage. Readbacks:
  `/home/kit/cnc-backups/mapper-autotop-f_vl8pdz/round-trip-readback.json` and
  `interrupted-readback.json`. See [the acquisition runbook](automatic-stock-mapping.md#automatic-top-map).
  UI activation and physical observation remain open; no restart or motion was
  issued. V2 surface requests now consume the retained coarse misses through
  [shared no-contact support](#retained-no-contact-support), excluding detected
  gaps from material checks and reconstruction without inventing surface hits.
  The standard command binary is built and installed; source identities and
  historical request replay survive the numerical file workflow. Still
  outstanding: establish the physical error model, distinguish inferred empty
  regions from unknown volume in the broader stock model, and execute reviewed
  additional spatial samples from material requirements. New top-column
  selection is now in the standard Object Mapper path; see the spatial-observation
  runbook above. The top acquisition and these
  constraints remain parts of the full workflow, not physical acceptance.
- [ ] **Exponential edge bracketing, then binary refinement.** Source and
  standard capture binary now use versioned policy V4 for new runs: **1 mm,
  2 mm, 4 mm, …** offsets from the initial top sample, in physical RIGHT /
  LinuxCNC -X, bounded by the retained plate/travel intersection. The last
  contact and first miss define the bracket; binary refinement stays inside
  it until the existing handoff distance is met. A plate-boundary contact
  remains an error with no inferred outside point. Old policy snapshots retain
  their original plate-first path. Numerical checks reported no failures for
  refinement, plate termination and historical replay. Physical behavior under
  this new policy has not been observed; that acceptance milestone remains open.
- [ ] **Growing local edge search with internal refinement.** V4 source and
  installed standard binaries now share replay in `probe_data/mapper_trace`.
  Coarse local radii double; half-radius contacts measure chord disagreement.
  Failed intervals halve using the already measured midpoint. Refinement
  retraces retained clear segments, preserves full backoff and original fine
  triggers, and exports selected contour order separately from all trial data.
  Initial / minimum trace interval is the sampling floor; Outline resolution
  is the midpoint error criterion. Reaching the floor retains an unresolved
  decision rather than claiming tolerance. Existing speed/depth policy and
  plate bounds remain in force. X labels remain physical RIGHT / LinuxCNC -X
  and physical LEFT / LinuxCNC +X. Seven mapper numerical checks reported passes.
  A synthetic concave-ledger round trip through installed export/import/
  prepare-stock retained all 67 fine contacts: 41 fit, one seam check and 25
  observations, with 42 selected records in contour order and 45 refinement
  decisions. FreeCAD exchange retained the source ledger and companions byte
  for byte. Readback: `/home/kit/cnc-backups/mapper-adaptive-v9ynzhbh/round-trip-readback.json`.
  These are numerical/file results. Physical tracing, reconstruction quality
  and the running UI's new label remain unobserved; this item stays open.
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
  `object_map/capture.rs`, `probe_data/mapper_trace/state.rs`, and
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
  passes; these do not establish machine behavior. V4 now refines the local
  next-contact interval from measured midpoint disagreement and carries those
  decisions through the shared capture context into stock/FreeCAD data. Broader
  stock-analysis-driven observation selection, multi-height acquisition and
  material containment remain outstanding. `check-material` now consumes a
  machining-footprint candidate and retained 3D surfaces to produce source-
  triangle-specific regions needing measurements. Full 3D covers distinguish
  local shortages, missing spatial support and independent-check issues; the
  source analyses and original triggers survive together. These regions still
  need conversion into bounded, supported acquisition plans; they are not probe
  targets or authorized hardware actions.
  The material-directed observation selector now maps patch check/shortage
  requirements to bounded **original** acquisition requests, shares the typed
  request/cycle reader and retains source companions exactly. Its normal
  catalog, editor, inspection and export path is built and installed; see the
  follow-up-observation runbook above for file readback and limits. Still
  required: planning entry/recovery paths, multi-height acquisition, and connecting reviewed
  proposals to the standard script executor with fresh exact capture/readback.
  New spatial top-column selection now consumes unsupported material regions,
  uses an explicit source-aligned grid and preserves the original acquisition
  envelope/settings. The standard binary/catalog/editor/inspection/export path
  is built and installed. Original hit/miss columns are not selected again;
  source snapshots and every unresolved region survive. Numerical/file details
  are recorded in the spatial-observation runbook above. New top proposals do
  not supply side/base access or measured material coverage.
  Historical Surface/Rim modes retain their cuboid fit for replay. The new
  `FreeSurface` mode provides automatic connected top coverage without that
  completion condition, and the outline path retains its free shape.
  Gauge-block cuboid measurements retain their intended role. New spatial
  sampling driven by material requirements now has a typed selection path;
  reviewed follow-up execution and fresh capture still need to connect those
  proposals back to the estimator.
- [ ] **Fit measured 3D stock surfaces and retain their support.** The standard
  binary and Object Mapper catalog now provide `prepare-stock-surface` and
  `fit-stock-surface`, with the existing editor/inspection/export path. Focused
  Rust modules estimate local Huber planes from retained 3D fine contacts,
  orient normals from approaches and apply shared calibration plus normal-based
  ball-radius correction. All original rows, ambiguous geometry and independent
  checks remain retained. Local check support is explicitly a planar
  interpolation assumption, not closed stock volume. Twenty-nine object-map
  numerical checks report passes. The installed-binary file exercise retained
  all 26 fine triggers and source bytes, produced 25 local patches and reported
  the withheld 2 mm disagreement without fitting that check. The request output
  newline defect exposed by this workflow was corrected at the standard entry
  point. Readback: `/home/kit/cnc-backups/mapper-surfaces-amm3v64y/round-trip-readback.json`.
  These are source, build and file milestones. Physical surface observations,
  volume coverage, uncertainty and containment remain open. The existing CNC
  session was not restarted.
- [ ] **Build the measured surface mesh and resolve stock volume.** The
  standard `prepare-stock-mesh` / `reconstruct-stock-mesh` operations now extract
  a source-linked mesh from the retained 3D patches, sharing local support and
  check interpretation with material comparison. Explicit lattice bounds,
  spacing and budgets produce indexed triangles, unresolved cells, unsupported
  facet records and measurement-region needs. Original trigger sources remain
  together through normal inspection/export. The installed-binary synthetic
  file exercise retained the expected 16 mm² top, 128 facets and open perimeter;
  source/export bytes matched and material replay remained unchanged. Forty-nine
  object-map numerical checks reported passes. See the reconstruction runbook
  and `/home/kit/cnc-backups/mapper-stockmesh-uery5t54/round-trip-readback.json`.
  Still required: top/side/base connections and crease handling, distinction of
  supported empty space from unknown volume,
  multi-height acquisition and the physical material interpretation. The new
  volume-placement path checks closed-shell geometry including vertex links and
  triangle intersections; that model does not establish physical occupancy. Open edges
  must not be silently capped, and edge incidence alone must not accept stock
  containment. Connect the measurement regions to acquisition and the resulting
  material model to placement/CAM; mesh export is a milestone within that flow.
- [ ] **Optimize machining placement inside measured stock.** Fit the unchanged
  required geometry into the stock estimate using the allowed translations and
  rotations. Account for material shortage and unknown coverage separately;
  expected oversize must not pull stock surfaces onto finished-part surfaces.
  Report local deficits even if an aggregate penalty is small. Select any
  clearance/allowance objectives from actual setup context. For dice OP1, preserve
  `stage1_after`, including backing and envelopes; checking only finished blanks
  would omit required intermediate material. The Huber STL registration
  objective remains separate. `prepare-footprint` / `fit-footprint` now connect
  a retained outline to an unchanged required-operation STL, search allowed XY
  translations and yaw, and retain worst local clearance/deficit bounds across
  complete projected triangles. Original captures, source analysis, STL and
  triangle identities are preserved through inspection/export; named locations
  can use the unreviewed horizontal candidate. The standard binary is built and
  installed. A synthetic file run improved its conservative horizontal clearance
  from -2.199750 mm to +0.597026 mm and retained every source file. Thirty-five
  object-map numerical checks report passes. Full 3D containment, integration
  with supported volume, actual setup constraints and physical acceptance remain
  open; neither the horizontal result nor these checks establishes them. See
  the footprint runbook and the retained file readback above. The standard
  `prepare-volume-placement` / `fit-volume-placement` path now adds bounded XYZ
  translation/rotation search against a reproduced, explicitly enclosed stock
  model, without resizing required geometry. It shares search mechanics and the
  candidate reader with footprint/material/scene operations. The numerical
  ellipsoid file exercise retained full source bytes and local deficits through
  CAD export. See the 3D placement runbook above. Partial-boundary interpretation,
  crease/base connections, setup constraints and physical acceptance remain open.
- [ ] **Use 3D material comparisons to drive placement and observation.**
  `prepare-material-check` / `check-material` are in the standard catalog and
  installed binary. They connect unchanged operation geometry to local measured
  surfaces, preserve every eligible local comparison and produce measurement
  regions tied to original triangles. The synthetic normal-CLI exercise retained
  1,920 full 3D regions, 26 original fine contacts and all source bundle bytes;
  534 regions showed local clearance shortages and 928 lacked support. Forty-one
  object-map numerical checks reported passes. See the material-check runbook
  and `/home/kit/cnc-backups/mapper-material-prtkle5x/round-trip-readback.json`.
  These are source/build/file milestones. The shared candidate path now accepts
  3D volume-placement results and exports their exact stock revision with the
  material scene. Closed reconstructed boundaries can drive the bounded 3D
  optimizer under an explicit enclosed-material assumption. Partial observations
  still need supported-empty/unknown-volume interpretation and conversion of
  measurement regions to acquisition plans. Physical observation and acceptance
  remain outstanding. Local inwardness is not containment.
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
  handling remain outstanding. The volume-placement stock path now adds
  adaptive-predicate triangle intersection checks and vertex/shell topology
  checks; other registration paths retain their stated mesh-validation limits.
  Edge closure alone does not establish absence of self-intersections.
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
  remain outstanding. `export-stock-scene` now supplies a combined retained
  material/stock scene with exact common-source binding, separate geometry
  roles and an explicit identity import convention for already transformed
  machine-mm meshes. The installed command's synthetic export retained all 50
  source files unchanged and rejected an altered source before publication;
  readback is in `/home/kit/cnc-backups/mapper-scene-58tfebd7`.
  This scene and the STL/ASC exchange are available; native Job updates and CAD
  workstation readback are not implemented/observed. OP1 must preserve
  `stage1_after`, including its
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
