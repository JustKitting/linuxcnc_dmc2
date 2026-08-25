# Pendant live signal mapping

These mappings were measured from the soldered Nano interface with the
monitor-only D4-D11 continuity scanner. They are observations, not assignments
copied from a wiring diagram.

| Physical selector position | Measured Nano connection | Status |
|---|---|---|
| Y + x1 | D5-D9 | Confirmed stable |
| X + x1 | D4-D9 | Confirmed stable |
| Z + x1 | D6-D9 | Confirmed stable |
| 4 + x1 | D7-D9 | Confirmed stable |
| 5 + x1 | D8-D9 | Confirmed stable |
| 6 + x1 | No D4-D11 connection | Confirmed stable; no connected axis-6 line observed |
| X + x10 | D4-D10 | Confirmed stable |
| X + x100 | D4-D11 | Confirmed stable |

The X/Y transition briefly reported no connection between detents, then
settled at the new pair.

## Other inputs

| Physical control state | Measured Nano state | Status |
|---|---|---|
| E-stop released | D12 LOW | Confirmed |
| E-stop pressed/latched | D12 HIGH | Confirmed |
| Side button held at X/x100 | D4 and D11 both LOW | Confirmed; grounds the selected axis/multiplier pair |
| Side button released at X/x100 | D4 and D11 both HIGH; D4-D11 continuity remains | Confirmed |

## Handwheel

| Physical action | Measured sequence | Status |
|---|---|---|
| One clockwise detent, viewed from pendant front | D2/D3: 00 -> 10 -> 11 -> 01 -> 00 | Confirmed; 2 edges on each pin |
| One counterclockwise detent, viewed from pendant front | D2/D3: 00 -> 01 -> 11 -> 10 -> 00 | Confirmed; 2 edges on each pin |

D2 leads D3 for clockwise rotation.
D3 leads D2 for counterclockwise rotation.

## Confirmed control policy

- Side button held: jogging is permitted after a fresh count baseline.
- Side button released: the outstanding jog target is cancelled and wheel
  changes are discarded.
- This is separate from the pendant E-stop input.
- Any selector or side-button transition is immediately reported as invalid,
  then must remain stable for 20 ms. Partial wheel motion is discarded when
  the transition begins and when the new state is accepted.
- Either E-stop edge discards pending/partial wheel motion before its P3 packet
  is published.
