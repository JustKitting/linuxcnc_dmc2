# Pinned external source

`linuxcnc-2.9.10/` is the unmodified official LinuxCNC source checkout at
commit `86cdca76fa2a36274c432caa21952b23c267989a`. The Rust build and offline
validator compare it with the installed 2.9.10 headers before accepting any
numeric interface catalog or task-status ABI.

The checkout is deliberately excluded from this repository because it retains
its own upstream Git metadata. Its exact version and cleanliness are enforced
by the build.

The checkout remains pristine. Reviewed post-release driver and tool-data fixes are stored
as provenance-locked overlays under `../patches/linuxcnc-2.9.10/`, applied to
an ephemeral archive by `../scripts/build_linuxcnc_driver_overlays.sh`, and
compiled against the installed 2.9.10 development contract. The overlay build
never edits this checkout.
