# Known plate reference from the 2026-09-12 recording

The reusable record is [known-plate-position.json](../config/metrology/known-plate-position.json).
It retains the measured local Y edge and the mounted-probe surface contact datums
for future model and script generation. It is a **partial measured reference**;
unmeasured coordinates are `null`, never zero or a machine travel boundary.

[plate-contacts-2026-09-12.csv](../config/metrology/plate-contacts-2026-09-12.csv)
retains all ten touch rows with their original numeric strings. The JSON also
retains the preceding motion frame for every touch. The original recording is
`tmp/output/probe/probe-1789234105598865762-845368-1.csv`;
SHA-256 `02d8dee2d4a7e232bca2d9b133f529ea3f885450048194481f059db93a3893cf`.
It reports 164030 frames, ten touches, no missing samples, and `incomplete=false`.
No contact was excluded.

The coordinates are LinuxCNC **machine XYZ in mm**, from `joint.N.pos-fb`
sampled on the IN1 rising edge, with all axes homed. They are trigger-sample
coordinates, not later stopped positions and not a hardware encoder latch.
Physical RIGHT / LinuxCNC -X; physical LEFT / LinuxCNC +X.

## Probe and local edge

The [product listing](https://www.amazon.com/dp/B0CC5BFFZW) and
[manufacturer title/specification](https://pgfuntransmission.com/product/npn-nc-cnc-3d-touch-probe-with-6-mm-shank-and-2-0-mm-tungsten-steel-ball-tip/)
specify a **2.0 mm ball tip**, distinct from the **6 mm shank**.
The nominal radius is `2.0 / 2 = 1.0 mm`. The legacy side-probe calibration
in `live/dmc2.ini` describes different equipment and is not applied here.

Contacts 1–4 approach in LinuxCNC -Y at machine X **293.526025 mm**.
Their trigger Y operands are `[166.83914044189453, 166.84785314941405, 166.84791928100586, 166.84896670532225]` mm.
The median trigger Y is **166.847886215 mm**; observed spread is
**0.009826263 mm**.

For a Y-normal outside edge, the nominal correction is:

`edge Y = median(trigger Y) - ball radius = 166.847886215 - 1.0 = 165.847886215 mm`.

This places the nominal surface on the -Y side of the ball center. The expression
assumes the ball center lies on the spindle XY axis and does not include a
calibrated eccentricity or trigger-pretravel correction. The general expression
including that correction is retained in the JSON. Sampling at one X does not
establish the edge's yaw.

## Surface contacts

| Contact IDs | Machine X, mm | Machine Y, mm | Probe-contact machine Z, mm | Height relative to first site, mm |
| --- | ---: | ---: | ---: | ---: |
| 5,6,7 | 293.526025 | 163.892320 | 35.141495 | +0.000000 |
| 8 | 299.994025 | 163.892320 | 34.970526 | -0.170969 |
| 9,10 | 299.994025 | 159.327320 | 35.143888 | +0.002393 |

Each row uses the median of all contacts at that XY. The single lower observation
at the second site is retained; its cause is not established. These points do not
establish that the plate is a plane. Relative heights subtract the first site's
median, so the unchanged probe's unknown constant Z offset cancels. Absolute
physical surface Z requires the installed ball-center/tool offset; subtracting the
ball radius alone from carriage Z would not supply it.

## Use in models and scripts

Read `local_y_edge.nominal_surface_machine_y_mm` for the recorded nominal local
edge and `top_surface_contacts.sites` for the contact datums/relative heights.
Preserve the nominal-calibration status in generated models. The X bounds,
opposite Y edge, yaw, thickness, and complete CAD-to-machine transform are not
measured in this file. Consumers must handle those `null` fields explicitly.
No work offset, tool table, machine configuration, or running session is changed
by saving this reference.
