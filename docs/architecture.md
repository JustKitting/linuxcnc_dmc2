# DMC2 LinuxCNC architecture

## Runtime data flow

```text
MYST1474 pendant
  -> Arduino Nano P3 serial protocol
  -> dmc2-serial-bridge (bounded parser + coherent HAL snapshot)
  -> dmc2_rt.so (1 kHz realtime policy + native wheel-jog counts)
  -> LinuxCNC servo-thread motion consumer and activity feedback
  -> Mesa 7I95T

LinuxCNC NML status
  -> task_status_shim.cc (version-locked C++ snapshot)
  -> dmc2-task-monitor (diagnostics + coherent HAL snapshot)
  -> dmc2_rt.so

LinuxCNC emcError queue
  -> error_channel.cc (the sole NML queue reader + version-locked raw snapshot)
  -> dmc2-task-monitor (total classification + synchronized checksummed journal)
  -> dmc2_axis/error_journal.py (validation and presentation only)
  -> AXIS notifications

LinuxCNC session launch
  -> dmc2-session-supervisor (Linux child subreaper and session-status owner)
  -> unchanged LinuxCNC 2.9.10 /usr/bin/linuxcnc
  -> dmc2-process-supervisor (direct owner for configurable long-lived processes)
  -> unchanged milltask / io / halui / axis / DMC2 userspace adapters
  -> var/log/linuxcnc/process-lifecycle.tsv

AXIS File Open / programmatic file path
  -> dmc2ctl Rust script-contract parser (read-only inspection)
  -> versioned typed contract + content revision
  -> stock AXIS load or dmc2ctl native load
  -> LinuxCNC returned loaded-file identity

Explicit AXIS Run / dmc2ctl execute-file
  -> active script prerequisites + exact loaded-file check
  -> LinuxCNC program-run consumer
  -> existing task-monitor error/recovery path
```

LinuxCNC is the only Mesa owner. No Python process is in the live motion,
limit, E-stop, watchdog, or spindle-command loop.

The error queue has exactly one consumer. AXIS releases its stock Python NML
error reader before polling the Rust-owned journal, so two processes can never
advance the same queued channel. The Rust journal retains the complete native
object, including type, declared size, serial number, operator ID, payload,
padding, transport state, and every uninterpreted byte. AXIS does not infer or
control machine state from that journal; it validates and displays records.

## Source responsibilities

- `rust/crates/dmc2-core`: pure deterministic policy. It knows pendant and
  machine state, but no HAL, serial, NML, or GUI APIs.
- `rust/crates/dmc2-rt`: LinuxCNC realtime component lifecycle, HAL pin
  registration, coherent input transport, and output publication.
- `rust/crates/dmc2-serial-bridge`: bounded P3 parsing, serial lifecycle, and
  coherent read-only pendant HAL publication.
- `rust/crates/dmc2-task-monitor`: native status and sole error-queue NML
  lifecycles, source-backed total classification, transition diagnostics,
  durable error journaling, and coherent status HAL publication.
- `rust/crates/dmc2ctl`: typed control operations for an already-running
  LinuxCNC session plus the authoritative arbitrary-file script-contract
  parser and acknowledged load/run path. The exact file format is
  `docs/script-contract.md`.
- `rust/crates/dmc2-process-supervisor`: passive, data-catalogued lifecycle
  ownership for the LinuxCNC session and configurable long-lived processes.
  It retains exact child ownership, compact process identity, kernel `wait4`
  status and resource usage, configured core limits, and matching LinuxCNC
  task backtraces. The session owner tees LinuxCNC stdout and stderr into the
  service journal and separate files; a nonzero session exit produces an
  automatic `/tmp/linuxcnc.report` containing the captured streams, command,
  and status. It never restarts LinuxCNC, changes machine state, or sends a
  signal to a tracked process. See `docs/process-lifecycle.md` for the exact
  evidence boundary and limitations.
- `rust/crates/dmc2-linuxcnc-interface`: generated, version-locked values and
  layouts consumed by the task monitor, including every public header, enum,
  integer macro, status object, and error-message object in the pinned source
  contract.
- `rust/crates/dmc2-launcher`: the standard compiled live-launch boundary;
  byte-exact profile/deployment checks, process-owner exclusion, persistent
  service creation, direct LinuxCNC process replacement, source-located HAL
  pin/signal conflict detection, and propagation of the persistent service's
  real terminal status and automatic failure report.
- `config/linuxcnc-driver-overlays.tsv` and `patches/linuxcnc-2.9.10`: the
  exact base version, upstream provenance, patch digest, build entry point,
  and deployment identity for reviewed post-release driver hardening. The
  pristine vendor checkout is archived into a temporary build tree and is
  never edited.
- `rust/crates/dmc2-hal-sys`: generated LinuxCNC HAL FFI declarations plus
  source-pinned return-code, lifecycle, and signal-link semantics.
- `config`: reviewed machine constants consumed by compiled production code.
- `python/dmc2_axis`: presentation-only AXIS integration required by AXIS. Its
  modules own notification handling, pendant-mode visibility, strict decoding
  of Rust script-inspection output, stock File Open/Run routing, and the
  special AXIS entry point as separate responsibilities. It does not parse the
  script header or implement a second LinuxCNC command protocol.
- `live`: the single accepted hardware profile and its NC programs.
- `archive/local`: untracked one-off experiments retained only for traceability.
- `artifacts/captures`: untracked raw hardware captures.
- `var/log` and `var/tmp`: runtime diagnostics and disposable project files.

## Dependency rules

1. Core policy cannot depend on LinuxCNC, HAL, serial, GUI, filesystem, or
   process APIs.
2. FFI and operating-system ownership stay at adapter boundaries.
3. Realtime code performs no allocation, blocking I/O, logging, or Python
   calls in the servo callback.
4. Public HAL schemas are compatibility contracts; the real LinuxCNC
   acceptance must successfully register and connect the production pins.
5. Every live HAL net has an explicit direction annotation and
   no pin may belong to two signals. A signal may have no more than one
   `HAL_OUT`; `HAL_OUT` and `HAL_IO` may never share a signal; multiple
   `HAL_IO` pins are valid tri-state writers.
6. Userspace snapshots use odd/even generations; realtime consumers accept
   only a matching, even generation before and after a read.
7. LinuxCNC numeric values and native motion pin semantics come from the pinned
   2.9.10 source tree at commit
   `86cdca76fa2a36274c432caa21952b23c267989a`, never from memory or duplicated
   guesses.
8. Live configuration cannot reference `reference`, `archive`, `artifacts`,
   or `var/tmp`.
9. Every configurable long-lived process has a direct status-owning parent,
   and the launcher-level child subreaper retains statuses for orphaned
   `linuxcncsvr`, `rtapi_app`, and owner descendants.
   The catalog defines ownership and identity; it does not claim that a
   zero-exit child received no caught signal.
10. A LinuxCNC driver overlay must name an exact release commit and upstream
    commits, pass its recorded SHA-256 check, compile against the installed
    version, retain the installed module's export set, and deploy in the same
    rollback transaction as the custom realtime modules.

## Build and evidence boundary

Formatting and compiling the Rust workspace establish only source and compiler
acceptance. The project contains no offline motion-acceptance harness. A build
does not prove launch, HAL registration, routing, LinuxCNC motion-consumer
acceptance, Ethernet delivery, physical step-pin output, drive response, motor
movement, limit behavior, recovery, UI survival, or spindle behavior. Each
boundary requires its own evidence, and hardware observation requires an exact,
explicitly authorized action.
