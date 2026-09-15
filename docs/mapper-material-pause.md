# Material shortage pauses the job

`Check candidate material` saves the original inputs, residuals and shortage
locations before returning an `insufficient-material` pipeline error when its
checked clearance bounds or retained clear sweeps identify missing material.
A clear sweep wholly inside the required material is also a shortage.
Conflicting evidence produces a separate pause reason. Unmeasured coverage
remains unresolved and cannot authorize cutting.

Open **Inspect analysis** in Object Mapper and select the saved analysis. Its
**Pipeline decision** tab contains the pause reason and references to the
original region, triangle and measurement records. New analyses also retain
`pipeline-state.json`. Old analyses are interpreted from their original
calculations; their bytes are not rewritten.

To resolve a shortage, select sufficient stock or explicitly revise the
placement, then run a new material check. The check never shrinks the required
geometry, relaxes clearance or changes placement automatically. Recovery uses
new evidence; dismissing an error does not establish sufficient material.

CAD scene exports remain available for inspection and carry the same paused
pipeline decision. They are not native CAM Jobs or cutting approval. The
current local-surface assessment cannot establish full stock occupancy, even
when every local comparison clears its requested allowance.

The pause belongs to the offline job. It does not own machine state, disable
Clear Fault, block Pendant Mode, apply offsets or issue machine commands.
