# Reviewed machine configuration

This directory contains small, source-controlled machine constants consumed by
the compiled controller. It is the production source of truth for values that
are not native LinuxCNC INI settings.

`machine-pulses.conf` records the user-confirmed motor DIP resolution and the
finite pendant/bounce target-distance and target-issuance policies. The Rust
core refuses an inconsistent or unreviewed relationship during compilation.

Physical jog velocity and acceleration are not duplicated here. The core
build reads those planner limits directly from `live/dmc2.ini` and derives
bounded completion and stop deadlines from the accepted live profile.

`operations.tsv` defines reusable operator actions and their compiled
execution contracts. `processes.tsv` defines every tracked LinuxCNC process's
exact program, launch site, ownership topology, criticality, backtrace policy,
INI-argument placement, core-dump capture policy, and the collision-checked
kernel process name used to identify an owner even after it has become a
zombie. It also defines the rolling live-process snapshot interval used to
retain resource and open-file evidence that Linux removes when a process
becomes a zombie, plus the caught-signal evidence contract selected for each
process. The caught-signal transport is catalogued as a contract, while its
runtime journal explicitly identifies the nonblocking delivery records as
best-effort rather than treating absence as proof.
`linuxcnc-driver-overlays.tsv` pins each local driver hardening patch to the
exact LinuxCNC release commit, upstream commit provenance, patch hash, build
entry point, staged artifact, and installed destination. These files are
embedded into the compiled launcher or owner so
a changed runtime contract requires a rebuild.
