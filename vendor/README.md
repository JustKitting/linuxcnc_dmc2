# Pinned external source

`linuxcnc-2.9.10/` is the unmodified official LinuxCNC source checkout at
commit `86cdca76fa2a36274c432caa21952b23c267989a`. The Rust build and offline
validator compare it with the installed 2.9.10 headers before accepting any
numeric interface catalog or task-status ABI.

The checkout is deliberately excluded from this repository because it retains
its own upstream Git metadata. Its exact version and cleanliness are enforced
by the build.
