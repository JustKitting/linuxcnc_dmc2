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
