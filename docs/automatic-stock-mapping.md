# Automatic rim trace

`Automatic rim trace` is the single automatic outline entry in Scripts. The
duplicate automatic block-map entry and file have been removed. Start above the stock as before.
It retains the original fine top trigger, brackets only the first edge in
physical RIGHT / LinuxCNC -X, then approaches the known inside point in physical
LEFT / LinuxCNC +X at the Z of the most recent successful top contact minus the selected Trace
depth. That depth defaults to the operator-requested 12.7 mm.

`config/mapper-outline.txt` owns the 25.4 mm initial bracket handoff requested by
the operator. Each run retains its own policy snapshot. Trace step uses the
existing editable spacing value as its local search radius. Outline resolution
controls the polygonal search circle's maximum chord sagitta and the final
seam-contact tolerance; sector count follows from those two values, with sectors
no larger than a quadrant. There is no fixed retry-count fault threshold.

After every coarse and fine edge contact, withdraw by the retained probe ball
diameter (currently 2.0 mm) along the reverse approach. Each release and recontact
is retained; only arrival at the full endpoint with input released allows the
next approach. A release transition alone is not a completed backoff. Probe successive candidate chords around
that contact at the same Z. Misses advance the local search; a collision sets
the next contour point and its observed approach direction. This walks around
convex and concave corners without first assuming a rectangle or visiting the
opposite side. No Z lift is inserted between local outline stations.

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
`dmc2.tactile-outline.v1` and contains ordered original fine machine-coordinate
XYZ contacts, the fixed work-coordinate Z plane, closure status and any partial
result reason. It applies no rectangle fit or nominal ball-radius correction.
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

The standard binary/configuration must be reopened after installing the matching
plan fields. Software checks are not evidence that a physical trace has completed.
