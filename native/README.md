# Native LinuxCNC control path

The live machine profile uses three compiled components:

- `dmc2-serial-bridge`: bounded Nano P3 parsing and coherent HAL publication.
- `dmc2-task-monitor`: read-only native LinuxCNC status, real task heartbeat,
  source-derived code/error classification, transition logging, and HAL
  diagnostics.
- `dmc2_rt.so`: the no-`std` servo-thread supervisor, limit/bounce policy, and
  finite HALUI command-edge generator.

`scripts/build_native.sh` runs the complete hardware-free verification suite,
builds release artifacts, verifies the realtime module ABI, and stages the two
userspace binaries under `native/bin`. `scripts/install_native_module.sh`
copies only the exact staged realtime module to LinuxCNC's module directory
and verifies the copy.

The build is locked to LinuxCNC 2.9.10 and its exact official source commit.
It refuses changed source or installed headers, generates 50 numeric code
domains (550 exact values), catalogs all 198 RS274 error templates, checks the
Rust/C++ status ABI, preserves all nine native NML transport error codes, and
handles all six error-channel message types. Run
`native/bin/dmc2-task-monitor --validate` to repeat the ABI/catalog check
without creating a HAL component or opening NML.

The live HAL contains no Python process with command authority. Python remains
only in presentation and offline validation tooling.
