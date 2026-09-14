# Automatic block outline

`Automatic block outline` in Scripts replaces the former automatic block map's
four-boundary/grid/cuboid sequence for new runs. Start above the stock as before.
It retains the original fine top trigger, brackets only the first edge in
physical RIGHT / LinuxCNC -X, then approaches the known inside point in physical
LEFT / LinuxCNC +X at the Z of the most recent successful top contact.

`config/mapper-outline.txt` owns the 25.4 mm initial bracket handoff requested by
the operator. Each run retains its own policy snapshot. Trace step uses the
existing editable spacing value as its local search radius. Outline resolution
controls the polygonal search circle's maximum chord sagitta and the final
seam-contact tolerance; sector count follows from those two values, with sectors
no larger than a quadrant. There is no fixed retry-count fault threshold.

After each retained fine edge contact, release along the reverse approach and
withdraw to the local search circle. Probe successive candidate chords around
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
50 mm/min fine touch/release, and 1500 mm/min clearance travel. A radial withdrawal
contact, missing local edge, repeated nonclosing region or mismatched seam
produces a readable partial-result error with Abort and Pendant Mode recovery.
Clear Fault retains its independent priority path.

CSV retains all captures and movement records. JSON uses
`dmc2.tactile-outline.v1` and contains ordered original fine machine-coordinate
XYZ contacts, the fixed work-coordinate Z plane, closure status and any partial
result reason. It applies no rectangle fit or nominal ball-radius correction.
Earlier ledgers keep their original mode, planner and interpretation; the
separate Automatic rim trace operation retains the earlier cuboid workflow.

---

The following describes the retained earlier mapper/rim workflow.

# Automatic stock mapping and rim tracing

The Scripts selector contains **Automatic block map** and **Automatic rim trace**.
Both start with the probe clear above the stock at the operator's chosen XY/Z.
The rim option discovers the footprint before approaching any side. X directions
are physical RIGHT / LinuxCNC -X and physical LEFT / LinuxCNC +X.

The plate envelope is `config/metrology/plate-envelope.txt`. The operator identified
the approximate X span as the full configured 0–300 mm range. Its two nominal Y
edges are -8.619127716064469 and 165.84788621520994 mm. The underlying measurements,
radius operands and raw recording are retained in
`config/metrology/references/plate-envelope-2026-09-14.json`; the earlier edge's
original record remains `config/metrology/known-plate-position.json`. X is an
operator-specified approximation; local Y contacts do not calibrate plate yaw.
The planner keeps the nominal ball inside this envelope and additionally checks
axis limits. It never uses the enlarged Y travel as a substitute plate boundary.

## Settings and procedure

The maximum downward budget defaults to 12.7 mm, including the initial air gap.
Every target stays between the original starting Z and `starting Z - budget`.
Starting Z is also the common clearance plane. The script never adds a second
12.7 mm below the first contact. Place the start above the stock's high point.

Probe reach defaults to **22 mm** from the PGFUN probe's
[new stylus specification](https://pgfuntransmission.com/wp-content/uploads/2024/11/6s-Nc.jpg),
measured in the drawing from the seating shoulder to the ball bottom. This is an
editable nominal specification default, as requested by the operator. Surface map,
Automatic block map and Automatic rim trace share one definition in
`config/script-panel.json`: `defaults.xyz-probe-reach-mm`. The existing 2 mm
reserve leaves a 20 mm reach budget; the separate 12.7 mm descent default is unchanged.
The descent budget plus the editable reserve must fit within the entered reach.
Saved unset zero values inherit this default; explicit nonzero settings remain editable.
Grid/station spacing defaults to the previously requested 1 mm; binary boundary
resolution defaults to 0.1 mm. Both are editable. The rim options additionally
set side ball-centre depth below the first top and clearance outside the fitted
edge. They default to the existing block scan's 1 mm depth and 2 mm clearance.

One runtime feed source, `config/mapper-feeds.txt`, sets 1500 mm/min clearance
travel, 800 mm/min horizontal search, 400 mm/min downward search and 50 mm/min
fine re-touch/release. Downward search was halved at the operator's request.
The versioned feed snapshot preserves older runs' original feeds. Each run snapshots
its feed data and plate envelope before measurement. Subsequent config edits
cannot alter an active run. Existing Surface map, Gauge block and Hole centering
retain their own settings and behavior.

The sequence is:

1. Double-touch the initial top within the original downward budget.
2. Probe the plate-bounded endpoints in four XY directions at starting clearance,
   dipping in Z at each location; binary-search each contact/no-contact bracket.
3. Visit an eight-connected outward grid until measured misses enclose it. Refine
   adjacent hit/miss crossings by bisection. All transfers return to starting Z.
4. Fit a rotated rectangular contact envelope. Independently re-touch inside and
   outside every fitted face midpoint and bisect those crossings.
5. For **Automatic rim trace**, use that footprint to double-touch stations on
   each planar side. Release and withdraw along the same normal before lifting,
   advance at clearance, descend outside the edge and touch the next station.
   Corner exclusion is derived from the ball radius and grid diagonal. The whole
   circuit is bounded before its first descent. It performs one circuit.

Both options finish at starting-Z clearance. They do not apply tool offsets,
work offsets, or CAD alignment. The initial rectangle is a contact envelope, not
radius-corrected stock dimensions. JSON separately reports a nominal stock-size
estimate: top-envelope spans minus the nominal ball diameter, or (for the rim
scan) the sum of opposing median side supports after subtracting the radius
once per face. The formula and assumptions accompany the estimate; original
trigger coordinates remain unchanged. An enclosing grid at the selected resolution does not measure
features smaller than that spacing or disconnected objects elsewhere on the plate.

## Records and recovery

`tmp/output/mapper/mapper-<timestamp>-<pid>.txt` retains the start settings, original
native G38 trigger doubles and bit patterns, declared approach, feed, movements,
misses and clearance returns. The shared M190 gate synchronizes storage and reads
each contact back before a release/backoff. Every published plan also has a
retained file. Completed runs produce CSV and JSON alongside the ledger; incomplete
geometry planning produces explicitly partial exports. Offline re-export uses
`native/bin/dmc2-probe-capture --export-mapper <ledger>` and never opens a machine
connection. Files from earlier runs are preserved.

A plate-boundary contact is not an outside miss or a measured stock edge. A fine
re-touch failure is not air. An input active without a G38 trigger stops with a
capture error. A planned upward withdrawal retains each detected G38 recontact
and release and continues toward the same starting-clearance Z at its existing
feed. These records do not become surface samples and do not cancel the scan.
The loop adds no sideways move, deeper target, retry count, or new fault latch.
It requires reported upward progress and a released input at the clearance
endpoint before allowing the next lateral move.

Unexpected horizontal transfer contact still stops after capture. An unexpected
downward Z contact outside an edge retains its trigger, releases vertically,
returns to starting clearance with the same upward-withdrawal handling, and
cancels. A release that cannot clear by its endpoint stops visibly. Generic
Abort adds no recovery motion.

Error messages identify the condition and direct recovery through Abort and
Pendant Mode. Each new Run creates a new ledger and invalidates old plan data.
Plan validity only guards consuming a motion target; it is not connected to Clear
Fault or Pendant Mode. The standard UI installs those controls before Scripts.
Clear Fault remains an independent, unconditional operator request.

The plan bank and new UI fields require reopening the standard CNC application
after installing the matching build. Compilation and offline geometry tests do
not establish physical probing behavior; no physical run is claimed by this document.
