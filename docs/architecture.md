# DMC2 LinuxCNC architecture

## Runtime data flow

```text
MYST1474 pendant
  -> Arduino Nano P3 serial protocol
  -> dmc2-serial-bridge (bounded parser + coherent HAL snapshot)
  -> dmc2_rt.so (1 kHz realtime policy and finite HALUI edges)
  -> LinuxCNC HALUI/motion
  -> Mesa 7I95T

LinuxCNC NML status
  -> task_status_shim.cc (version-locked C++ snapshot)
  -> dmc2-task-monitor (diagnostics + coherent HAL snapshot)
  -> dmc2_rt.so
```

LinuxCNC is the only Mesa owner. No Python process is in the live motion,
limit, E-stop, watchdog, or spindle-command loop.

## Source responsibilities

- `rust/crates/dmc2-core`: pure deterministic policy. It knows pendant and
  machine state, but no HAL, serial, NML, or GUI APIs.
- `rust/crates/dmc2-rt`: LinuxCNC realtime component lifecycle, HAL pin
  registration, coherent input transport, and output publication.
- `rust/crates/dmc2-serial-bridge`: bounded P3 parsing, serial lifecycle, and
  coherent read-only pendant HAL publication.
- `rust/crates/dmc2-task-monitor`: native NML lifecycle, source-backed status
  classification, transition diagnostics, and coherent status HAL publication.
- `rust/crates/dmc2-linuxcnc-interface`: generated, version-locked LinuxCNC
  2.9.10 numeric catalogs. It is the only source of numeric interface codes.
- `rust/crates/dmc2-launcher`: the standard compiled live-launch boundary;
  byte-exact profile/deployment checks, process-owner exclusion, persistent
  service creation, and direct LinuxCNC process replacement.
- `rust/crates/dmc2-hal-sys`: generated LinuxCNC HAL FFI declarations.
- `config`: reviewed machine constants shared by compiled production code and
  offline compatibility tests.
- `python/dmc2_axis`: presentation-only AXIS integration required by AXIS. Its modules own
  notification handling, pendant-mode visibility, and the special AXIS entry
  point as separate responsibilities.
- `live`: the single accepted hardware profile and its NC programs.
- `sim`: hardware-free LinuxCNC configuration.
- `tests`: integration tests and small deterministic fixtures.
- `reference`: preserved, non-live behavioral or historical implementations.
- `archive/local`: untracked one-off experiments retained only for traceability.
- `artifacts/captures`: untracked raw hardware captures.
- `var/log` and `var/tmp`: runtime diagnostics and disposable project files.

## Dependency rules

1. Core policy cannot depend on LinuxCNC, HAL, serial, GUI, filesystem, or
   process APIs.
2. FFI and operating-system ownership stay at adapter boundaries.
3. Realtime code performs no allocation, blocking I/O, logging, or Python
   calls in the servo callback.
4. Public HAL schemas are compatibility contracts and must have exact tests
   for names, types, directions, and uniqueness.
5. Userspace snapshots use odd/even generations; realtime consumers accept
   only a matching, even generation before and after a read.
6. LinuxCNC numeric values come from the pinned 2.9.10 source tree at commit
   `86cdca76fa2a36274c432caa21952b23c267989a`, never from memory or duplicated
   guesses.
7. Live configuration cannot reference `reference`, `archive`, `artifacts`,
   or `var/tmp`.

## Verification boundary

The offline build must format and compile every Rust target, execute the full
workspace tests, validate the LinuxCNC source/header fingerprint and status
ABI, validate every live HAL/INI/UI connection, and enforce the source-layout
limits. The launcher is additionally instrumented with the matching LLVM 19
tools and must have zero missed production regions, functions, or lines.
Hardware motion or a LinuxCNC restart is a separate explicitly ordered
operation and is never part of an offline verification command.
