# Gauge block scan

Select **Gauge block scan** in the pendant pane's **Custom Scripts** tab.
The operator establishes home and places the probe clear above the block before
pressing Run. Selecting the script or editing its fields issues no motion.
Nominal block dimensions and in-plane alignment are not required.

The initial top search defaults to the user-requested 25 mm maximum descent.
The script uses the existing 200 mm/min coarse and 50 mm/min fine contact feeds.
Each contact is retained before release and slow re-touch. The nominal XYZ probe
ball diameter is 2 mm, as recorded in `surface-mapping.md`; old electrical-probe
offsets are not applied to it.

An outward 1 mm grid records top contacts and bounded misses. Only hit cells
expand the eight-neighbour frontier. Search ends when every neighbouring cell
has been measured, giving a surrounding miss boundary. It does not fill in a
height for a miss, clip the scan at a travel boundary, or assume an axis-aligned
block. The grid dip below the first top contact defaults to 1 mm and is editable.

Rust fits a minimum-area rotated rectangle around the top contact envelope and
checks the retained miss cells for inconsistent interior gaps. This is an initial
footprint for side acquisition, not a precision dimension measurement. Side
stations exclude the corners by the nominal ball radius plus one grid diagonal.
Approach positions add the selected clearance outside that envelope; touches
travel inward along a face normal. X increasing means **physical LEFT / LinuxCNC
+X**; decreasing means **physical RIGHT / LinuxCNC -X**.

The perimeter is visited in adjacent-face order, at 1 mm station spacing. Three
circuits reuse the identical XY stations and approach directions, with the second
and third circuits respectively 1 mm and 2 mm lower. The panel's **First side
ball-centre depth** is measured below the first top plane. Commanded Z is:

`first fine top trigger Z - nominal ball radius - first centre depth - circuit offset`.

With the default 1 mm centre depth and 1 mm ball radius, those commanded planes
are 2, 3 and 4 mm below the first fine top trigger. This accounts for the ball's
centre being above its bottom during a top touch. A downward contact outside the
footprint may be the plate or another obstruction; it is not accepted as a side
sample or proof of which object was touched.

All side approaches and all three depth planes are checked against the configured
travel limits before the first side descent. Lateral transfers between stations
take place at the highest retained fine top contact plus the panel's **Clearance
above measured top**, default 2 mm. A side contact withdraws along its approach
normal before the vertical return to clearance.

## Unexpected Z contact and operator control

On contact during an outside-edge downward Z transfer:

1. Retain the original G38 trigger XYZ, source, frame, feed and movement endpoints.
2. Flush the ledger to storage and read the exact appended record back.
3. Withdraw vertically upward until release, then continue vertically to the
   clearance plane above the measured top. XY remains unchanged.
4. Retain the recovery endpoint and cancel the scan with a readable operator error.
   There is no next grid or side measurement in this run.

Capture/readback failure prevents the withdrawal and reports **CAPTURE FAILED —
KEEP THE SETUP IN PLACE**. A failed release or another contact during the upward
return stops that return and reports why clearance was not reached. No substitute
path, retry, fault clear or restart is issued. An ordinary operator Abort uses the
existing abort hook and does not start a recovery movement. The independent
**Abort**, **Clear Fault**, **Machine On** and **Pendant Mode** controls remain
the operator's recovery path; script-field locks do not gate those controls.

## Records and analysis

`tmp/output/block/block-<timestamp>-<pid>.txt` is a unique durable ledger. It
retains fine and coarse contacts, misses, ordinary movement endpoints,
obstructions and recovery events. Original trigger coordinates come from
`emcStatus.motion.traj.probedPosition` in machine millimetres; exact f64 bits and
the work-to-machine translation are retained. Reported endpoints are labelled
separately and never substituted for contact coordinates. Planned steps have
sequence-qualified files alongside the ledger.

An ordinary completed scan also writes CSV and JSON alongside its ledger. The
CSV includes commanded direction vectors and feed. Analysis fits an interior top
plane and side planes across the three circuits, retaining residuals. Border
ball contacts remain in the CSV but are excluded from the interior top-plane fit.
These fits describe relative block/probe/axis geometry. They do not independently
separate block placement, plate shape, probe deflection and spindle/axis error.
No calibration or offset is automatically applied.

The standard capture binary can export a retained partial ledger without a
machine connection using `dmc2-probe-capture --export-block <ledger>`. Partial
exports have sequence-qualified names and do not overwrite a full result.

The standard typed program is `live/nc_files/measure-gauge-block.ngc`. Rust
planning and analysis live in `dmc2ctl/src/probe_capture/block`. M190 saves
records and publishes planning data into AXIS-owned HAL parameters; it does not
issue machine commands. `config/block-plan-fields.txt` defines that data bank.
The next plan's sequence is published last and read only after the M190/M66
interpreter barrier. An old plan cannot be accepted for a different record count.

Offline tests exercise rotated-footprint planning, signed grid coordinates,
repeated stations, bounds, durable captures and terminal recovery. Those tests
are not evidence of physical scan or recovery behaviour.
