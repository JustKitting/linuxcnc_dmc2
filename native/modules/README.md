# Staged LinuxCNC driver overlays

`scripts/build_linuxcnc_driver_overlays.sh` writes reproducible, untracked
module artifacts here. The source-controlled catalog, patch, and build path
are `config/linuxcnc-driver-overlays.tsv`, `patches/linuxcnc-2.9.10/`, and the
build script itself.

The launcher compares each staged artifact byte-for-byte with LinuxCNC's
installed module and uses the existing transactional installer only while
`rtapi_app` is absent. Building an overlay does not install it, launch
LinuxCNC, or issue a machine command.
