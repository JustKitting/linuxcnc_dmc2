# Native LinuxCNC control path

The live machine profile uses three compiled components:

- `dmc2-serial-bridge`: bounded Nano P3 parsing and coherent HAL publication.
- `dmc2-task-monitor`: read-only native LinuxCNC status and real task heartbeat.
- `dmc2_rt.so`: the no-`std` servo-thread supervisor, limit/bounce policy, and
  finite HALUI command-edge generator.

`build_native.sh` runs every offline unit/integration test, builds release
artifacts, verifies the realtime module ABI, and stages the two userspace
binaries under `native/bin`. `install_native_module.sh` copies only the exact
staged realtime module to LinuxCNC's module directory and verifies the copy.

The live HAL contains no Python process with command authority. Python remains
only in presentation and offline validation tooling.
