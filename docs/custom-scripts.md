# Custom Scripts in AXIS

The wider pendant pane contains **Pendant** and **Custom Scripts** tabs.
The toolbar's existing **Pendant Mode** control shows that pane. Select
**Hole centering** or **Surface map**, edit the parameter fields, and use that
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

Both probing scripts retain their prior all-homed prerequisite, spindle-off
checks, travel-boundary checks, contact sequence, feeds, and M190 trigger
capture. The panel reports unmet machine prerequisites before submission.
It does not home, reset, clear a fault, move, or start a script merely because
the tab is selected or a value is edited.

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
numeric kinds, defaults, and increments. Python implements the stock AXIS/Tk
bindings only; the existing LinuxCNC programs and Rust capture/control path
retain machine execution and measurement ownership. The standard launcher
includes these source inputs in its build-matching check.
