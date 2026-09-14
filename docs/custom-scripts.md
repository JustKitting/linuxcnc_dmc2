# Custom Scripts in AXIS

The wider pendant pane contains **Pendant** and **Custom Scripts** tabs.
The toolbar's existing **Pendant Mode** control shows that pane. Select
**Gauge block scan**, **Hole centering**, or **Surface map**, edit the parameter fields, and use that
script's **Run** button. The +/− buttons increment a distance by 1 mm; the text
fields accept decimals. Valid values are retained in stock AXIS preferences.

The pane opens the reusable program through the existing Rust script-contract
inspector and stock AXIS loader, then uses the existing guarded Run command.
It does not edit a program or generate a temporary file. The programs snapshot
their named AXIS HAL parameters at entry. Invalid fields deassert the matching
parameters-valid pin and block that script. The Run/Step guard also holds the
fields during a submitted program, including programs selected through normal
File Open. Abort ends execution through the existing stock control; editing
unlocks after LinuxCNC reports an idle, acknowledged command state.

The probing scripts use an all-homed prerequisite, spindle-off
checks, travel-boundary checks, contact sequence, feeds, and M190 trigger
capture. The panel reports unmet machine prerequisites before submission.
It does not home, reset, clear a fault, move, or start a script merely because
the tab is selected or a value is edited.

The [gauge block scan](gauge-block-scanning.md) discovers a rotated block on a
1 mm grid and measures the same side stations on three circuits 1 mm apart.
An unexpected outside-edge Z contact is retained before vertical withdrawal
above the measured top and cancellation. Its clearance field sets that return
height; nominal block dimensions are not required.

**Go to Home** is in this tab. It uses the established machine home and sets
G61.1 exact-stop mode: the Z move finishes before X/Y starts, without blending
across the transition. Its return coordinates and speed remain the existing
values. **Home All**, which establishes home, remains in Manual Control and
homes Z fully, then X, then Y. The return remains unavailable while unhomed.

The **Probe recorder** section is also in this tab: Probe Mode controls touch
bubbles, Record controls retention, and Retry Save retains its existing failed
save recovery. Turning Record off saves; turning Probe Mode off also turns
Record off. The recorder's Rust acquisition/storage path and HAL wiring are
unchanged. These controls are created independently of script parameter
validation, so an invalid script field does not remove recorder-off controls.

`config/operations.tsv` identifies the programs and their typed effects and
prerequisites. `config/script-panel.json` defines the widget fields, HAL names,
numeric kinds, defaults, and increments. A default may be numeric text or a typed
`{"reference": "name"}` pointing to the catalog's `defaults` dictionary. Each
shared default retains its value, description and source. The probe reach fields
share `xyz-probe-reach-mm`, the PGFUN new stylus's 22 mm specification. A saved
unset zero for a positive field using a shared default inherits that value on
startup; nonzero preferences and invalid editing text are preserved for ordinary
field validation. Python implements the stock AXIS/Tk bindings only; the existing LinuxCNC programs and Rust capture/control path
retain machine execution and measurement ownership. The standard launcher
includes these source inputs in its build-matching check.
