# Object mapper scaffold / FreeCAD exchange

The [positional mapper runbook](positional-mapper.md) extends this store with
STL inspection, mapper-ledger import, local pose fitting, stock-face dimensions
and named coordinate exports. The [continuation research](probe-based-continuation.md)
describes FreeCAD, remaining-material modelling and additive continuation.

The standard Rust `native/bin/dmc2ctl object-map` command manages persistent
objects, named setups, immutable capture snapshots and design revisions. Its
dispatch occurs before any NML session is opened. These are file operations;
they have no motion, fault, mode, HAL, restart or machine-recovery transition.
The existing AXIS Pendant Mode and Clear Fault paths do not depend on object
records. Mapper input/storage errors report the file and correction/retry action
on the calling command's output, and do not create an AXIS error latch.

## Available scaffold

- Create and list named objects, with independent setups for later placements.
- Attach native FreeCAD `.FCStd`, STEP or STL files as preserved design revisions.
  Attachment copies the bytes; it does not parse or certify the CAD geometry.
- Import existing circle, surface, gauge-block or mapper G38 ledgers. Each snapshot
  retains the original file bytes and source path, including trigger f64 bits,
  coarse and fine touches, misses, travel, settings and result records.
- Export an individual setup to a new directory containing FreeCAD Points
  `.asc` files, contact CSVs, original ledgers, attached designs and JSON metadata.

This stage has no new AXIS pane. It establishes the data commands that a later
object-mapper pane can call through the standard binary.

## Commands

From the project root:

```sh
native/bin/dmc2ctl object-map --help
native/bin/dmc2ctl object-map create bracket 'Bracket blank'
native/bin/dmc2ctl object-map add-setup bracket initial 'Initial placement'
native/bin/dmc2ctl object-map import-capture bracket initial top /path/to/retained-ledger.txt
native/bin/dmc2ctl object-map attach-design bracket original /path/to/model.FCStd
native/bin/dmc2ctl object-map show bracket
native/bin/dmc2ctl object-map export-freecad bracket initial /path/to/new-export-directory
```

`[--store DIRECTORY]` immediately after `object-map` overrides the default
`var/objects` under the project. IDs are lowercase letters, digits, hyphens or
underscores. Labels may contain spaces and Unicode. No setup or object is
implicitly assigned to existing measurements: import names both explicitly.

The store layout is:

```text
var/objects/<object-id>/object.dmc2
var/objects/<object-id>/designs/<revision-id>.dmc2
var/objects/<object-id>/setups/<setup-id>/setup.dmc2
var/objects/<object-id>/setups/<setup-id>/captures/<capture-id>.dmc2
```

Records use versioned UTF-8 metadata followed by a blank line and, for snapshots,
the exact source payload. Relative object/setup/capture identities survive a
store transfer to another computer. Absolute source paths are provenance only;
reads and exports use the retained payload, never reopen those source paths.
Writes use the shared capture publisher: unique temporary file, flush, exact
readback and publication without replacing an existing different file. Repeating
the same record is idempotent; conflicting content requires a new ID. Neither
original measurements nor prior design revisions are silently replaced.

## Coordinate and evidence contract

ASC contains fine **original LinuxCNC machine G38 trigger coordinates in mm**.
Coarse contacts remain in CSV and the original ledger. Reference contacts are
included; the manifest maps every ASC row to its original ledger sequence.
Misses never become a zero-height or search-floor point. Raw obstruction events
remain in the original ledger. Direction columns describe the commanded
approach, and feed is commanded mm/min, not an observed velocity.

No ball-radius, spindle-to-ball, plate, object-frame or mounting correction is
applied. Thus the cloud is a **trigger envelope**, not a reconstructed stock
surface or a ball-centre cloud. Each capture has its own ASC: different captures
are not implicitly merged or assumed to share a mounting or machine reference.

Capture states are `partial`, `recorded-result-unreviewed`, or `quarantined`.
A terminal result means only that the source recorded one. This scaffold does
not independently accept the scan's coverage, feature fit or dimensions. An
obstruction or gauge-block side miss quarantines the capture: it can be retained
and inspected as ledger/labelled CSV, but it produces no ASC. Invalid schemas,
missing source labels, mismatched original trigger bits and torn records are
rejected rather than repaired or replaced by stopped positions.

Accepted setup registration is explicitly `unresolved`, with `object_to_machine: null` and
`residual_mm: null`. Probe uncertainty and reconstructed stock remain null;
`cam_ready` is false. An identity transform or zero uncertainty would invent a
measurement, so neither is a default. Setup labels are operator organisation,
not evidence that relocation or machining physically happened.

Named `analysis_candidates` now appear in each setup. Their presence does not
accept a placement. `export-fit` exports a specific proposal with its STL,
calibration request, source captures and residuals; the original `export-freecad`
command continues to export the unmodified trigger envelopes.

## FreeCAD handoff and next implementation

Open each `.trigger-envelope.asc` with **Points → Import points**. Open the
attached `.FCStd` or STEP design separately. Consult `manifest.json` for the
coordinate meaning and capture state before interpreting their overlap.
The manifest is published after all export payloads have been flushed and read
back. A directory without it is an interrupted export; retry with a new directory.

FreeCAD's documented foundations are its
[Points workbench](https://github.com/FreeCAD/FreeCAD-documentation/blob/main/wiki/Points_Workbench.md),
[CAM Job stock/orientation controls](https://github.com/FreeCAD/FreeCAD-documentation/blob/main/wiki/CAM_Job.md),
and [native document format](https://github.com/FreeCAD/FreeCAD-documentation/blob/main/wiki/File_Format_FCStd.md).
This scaffold does not execute FreeCAD or modify a CAM job.

Local STL fitting with explicit compensation and provenance is now available
through the positional commands. Reviewed placement acceptance, reconstructed
stock volume and integration that updates FreeCAD setup/operation inputs and
regenerates toolpaths remain future stages. No alignment acceptance tolerance,
sampling extent, scan speed or other new physical setting is introduced here.

Offline serialization and storage tests exercise data rejection, exact snapshot
retention, independent setups and no-overwrite export. They are not evidence of
physical probing, dimensional accuracy, machine recovery or FreeCAD CAM behaviour.
