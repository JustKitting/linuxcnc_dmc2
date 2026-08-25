# Reviewed machine configuration

This directory contains small, source-controlled machine constants consumed by
the compiled controller and validated offline. It is the production source of
truth for values that are not native LinuxCNC INI settings.

`machine-pulses.conf` records the user-confirmed motor DIP resolution and the
reference resolution used by the earlier accepted movement tests. The Rust
core refuses a non-integral or unreviewed scale during compilation.
