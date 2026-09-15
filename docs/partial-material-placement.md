# Placement from partial measurements

In Object Mapper, select **Prepare placement refinement** and a retained V3
material assessment. The draft inherits its design and unit conversion.
Clearance, surface allowance, support neighbourhood, normal band, cover radius
and the probe/error model come from that assessment; refinement cannot change
them. Enter correction bounds and computation budgets, save the request, then
select **Refine placement from measurements**.

Translations are corrections to the current model-origin position in machine
millimetres. Rotations are corrections about that model origin, composed as
`Rz(yaw) Ry(pitch) Rx(roll) R_original`. Every interval includes zero; `0,0`
freezes a coordinate. These are offline placement proposals, not axis commands.

The objective maximizes the minimum constraint slack in millimetres. It retains
every initially checked cover/patch comparison, including its finite lateral
support hull and neighbourhood distance. Leaving that support incurs a
negative slack instead of deleting the comparison. Clear paths are covered by
finite segment balls at the assessment's cover radius and compared with signed
outside distance to the unchanged required material. This also detects a clear
path wholly inside required material. No stock box or closed measured-stock
mesh is required. The normal band limits the final local assessment's evidence;
it is not a depth limit on placement below a measured surface.

The original placement participates in the search. The output includes its
comparison with the best bounded candidate, search history, original geometry,
original source observations, a transformed STL and `pose-candidate.txt`.
The source assessment remains unchanged. Refinement then recalculates the full
material assessment at the proposed placement, including previously unsupported
regions. Its `pipeline-state.json` remains paused for insufficient material,
conflicting evidence or unresolved coverage; numerical improvement does not
approve cutting.

To feed the result back into the workflow, run **Check candidate material** with
the retained `material-check-request.txt` and a new analysis ID. Use that
assessment for follow-up observation preparation or a CAD stock-scene export.
New observations can be assessed against the same candidate in a new surface
revision sharing the declared physical frame. The operation has no machine I/O
and does not change Clear Fault or Pendant Mode.
