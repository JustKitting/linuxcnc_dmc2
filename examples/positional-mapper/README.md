# Synthetic positional-mapper example

Run from the project root:

```sh
bash examples/positional-mapper/run-example.sh /tmp/my-positional-example
```

Choose a new output directory. This invokes only the standard Rust
`dmc2ctl object-map` file commands. No machine connection or motion occurs.

All inputs in this directory are synthetic. The intended cuboid measures
40 × 30 × 20 mm. Its known transform is yaw 7° and translation (70,60,40) mm.
The trigger fixture includes a nonzero mounting vector and pretravel correction,
independent check contacts, oversized X/Y stock faces, and no measured bottom.
Expected stock spans are 42 mm, 33 mm, and unknown Z. The source ledger has no
terminal scan result and remains labelled partial.

The example settings in `fit-request.txt` belong only to this numerical fixture.
The reusable [runbook](../../docs/positional-mapper.md) explains how to generate a
request from real retained captures, select references and supply actual
calibration and model units. The [research report](../../docs/probe-based-continuation.md)
covers CNC and additive continuation and the remaining implementation stages.
