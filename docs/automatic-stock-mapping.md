# Automatic rim trace

`Automatic rim trace` is the single automatic outline entry in Scripts. The
duplicate automatic block-map entry and file have been removed. Start above the stock as before.
It retains the original fine top trigger, brackets only the first edge in
physical RIGHT / LinuxCNC -X, then approaches the known inside point in physical
LEFT / LinuxCNC +X at the Z of the most recent successful top contact minus the selected Trace
depth. That depth defaults to the operator-requested 12.7 mm.

`config/mapper-outline.txt` owns the 25.4 mm initial bracket handoff requested by
the operator and the initial 1 mm edge-search offset. New runs use policy V4:
probe at offsets 1, 2, 4, 8, ... mm from the starting top sample in physical
RIGHT / LinuxCNC -X. These are distances from the initial sample, not cumulative
travel legs. Growth ends at the first retained miss or at the retained plate
boundary, whichever comes first. A plate-boundary contact is an error, not an
invented outside point. Binary refinement stays inside the last contact/miss
bracket until it meets the handoff distance, then the existing side approach
finds the edge at tracing depth. The newest successful top trigger establishes
that depth. Each run retains its own policy snapshot; V1 and V2 recordings
retain their original plate-first search during replay. V3 retains its fixed
local radius; V4 adds growing intervals and measured midpoint refinement.

Initial / minimum trace interval uses the existing editable spacing value as
both the starting radius and the minimum radius. From the last selected fine
contact, sample a coarse local circle, return along the retained clear segments,
then sample the half-radius circle. If that midpoint projects between the two
end contacts and its perpendicular chord error meets Outline resolution, select
midpoint then coarse contact in contour order and double the next radius.
Otherwise reuse the already measured midpoint as the smaller coarse trial and
halve again. Farther trials remain observations in the original ledger.

Refinement stops before halving below the minimum interval. The selected nearest
contact then carries an unresolved-spacing decision; it does not claim to meet
the error tolerance. The next search grows again. Radius growth is bounded by
the nearest retained plate/travel edge minus the full backoff, and by the closing
seam when approaching it. Search-circle chord sagitta also uses Outline
resolution, with sectors no larger than a quadrant. No fixed retry-count fault
threshold is added. These sampling decisions do not establish an unsampled solid.

After every coarse and fine edge contact, withdraw by the retained probe ball
diameter (currently 2.0 mm) along the reverse approach. Each release and recontact
is retained; only arrival at the full endpoint with input released allows the
next approach. A release transition alone is not a completed backoff. Successive
candidate chords stay at the same Z; misses advance the local search and hits
retain the original fine contact and observed approach direction. Refinement
reverses the same previously traversed clear segments and restores the selected
coarse release by those segments when necessary. It does not insert a shortcut
through unobserved space. This local construction assumes no rectangle or
opposite-side correspondence. No Z lift is inserted between local stations.

A nearby return with compatible approach direction is only a closure candidate.
After traversing more than the local circle's circumference, independently
re-touch the original seam and require agreement within Outline resolution.
Only then record a closed contour and perform the existing final upward return
to the starting clearance. Every contact is synced and read back before any
following move; reported released endpoints remain separate from trigger XYZ.

Feeds remain 400 mm/min downward search, 800 mm/min horizontal search,
50 mm/min fine touch/release, and 1500 mm/min clearance travel. An unfinished
withdrawal, missing local edge, repeated nonclosing region or mismatched seam
produces a readable partial-result error with Abort and Pendant Mode recovery.
Clear Fault retains its independent priority path.

CSV retains all captures and movement records. JSON uses
`dmc2.tactile-outline.v2` and contains selected original fine machine-coordinate
XYZ contacts and record identities in contour order, all fine trial contacts,
midpoint decisions, the fixed work-coordinate Z plane, closure status and any
partial result reason. Contour order can differ from acquisition order because
a midpoint is measured after the farther coarse trial. Invalid adaptive capture
cycles do not receive a guessed contour order. Failed legacy captures retain
explicitly labelled chronological diagnostic contacts. This export applies no
rectangle fit or nominal ball-radius correction.
Earlier ledgers keep their original mode, policy and interpretation for offline
replay. New runs use the below-contact policy and require full backoff evidence.


The maximum initial search descent applies to top discovery from the starting
height. Trace depth is additional below the retained top contact; it is not
subtracted again at each contour station. The complete descent from starting Z
to the tracing plane must fit the configured usable reach after reserve and
machine Z travel. An outside-edge downward contact retains its trigger, returns
up to starting clearance and cancels through the existing recovery path.

The plate envelope and nominal ball diameter come from
`config/metrology/plate-envelope.txt`. The operator supplied the approximate full
X span and the two recorded Y edges; their source values and radius operands
remain in `config/metrology/references/plate-envelope-2026-09-14.json`. Every
lateral endpoint, including diameter backoff, stays within that envelope and
axis travel. Probe reach retains the shared editable PGFUN specification in
`config/script-panel.json`. Saved settings for the retained rim entry are preserved;
the new depth field has its own preference key because its meaning changed.

Each run retains a versioned outline policy, feed and plate snapshot beside
`tmp/output/mapper/mapper-<timestamp>-<pid>.txt`. The ledger distinguishes exact
trigger coordinates from reported withdrawal endpoints; it retains feeds and
paths for both. The completion record is `withdrawal-complete`. New outline
cycles require it before the slow re-touch and before another local candidate.

The acquisition planner, capture export and stock preparation share retained-data
replay in `rust/crates/dmc2ctl/src/probe_data/mapper_trace`. `prepare-stock` selects
the replay's contour order, withholds the seam as a check and keeps every other
fine contact as an observation. Policy snapshots and refinement decisions follow
the capture into stock analysis and FreeCAD exchange; neither consumer substitutes
current configuration for missing historical context.

The same retained captures can be selected through Object Mapper's **Prepare 3D
stock surfaces** operation alongside top or multi-height side observations in
the same declared setup reference. The Rust estimator retains local normals,
ball-radius-corrected positions, all residuals and independent checks. A single
line of rim contacts leaves wall slope unresolved. Local interpolation support
and missing volume coverage feed the continuing reconstruction work; no stock
surface analysis launches another probing operation. See
[the surface runbook](positional-mapper.md#estimate-local-3d-stock-surfaces).

V4 uses the existing plan fields and executor and requires no new HAL pins. The
installed capture binary reads the policy when a new run begins. The running UI
may retain its earlier description until reopened. Numerical rotated/concave
fixtures and a file round trip exercised this source; neither is evidence of a
physical trace. No machine run or restart was performed for this change.

An explicit fine re-touch allowance remains pending; see
`docs/positional-mapper.md` for the continuing implementation list. Growing local
search does not correct the fine-endpoint failure retained in the earlier run.
