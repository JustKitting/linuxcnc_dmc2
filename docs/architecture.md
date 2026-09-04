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
- `rust/crates/dmc2-process-supervisor`: passive, data-catalogued lifecycle
  ownership for the LinuxCNC session and configurable long-lived processes.
  It retains zombie-safe process and parent identity, pre-reap `/proc` and
  cgroup evidence, independently checked `waitid`/`wait4` status, resource
  usage, configured crash-dump limits, matching LinuxCNC task backtraces, and
  identity-checked durable kernel-core copies for core-generating deaths. A
  narrowly scoped native interposer records the `siginfo_t` for caught
  SIGINT/SIGTERM deliveries to the catalogued LinuxCNC C consumers before
  those consumers convert the signal into a zero exit. That nonblocking record
  is explicitly best-effort. The session owner tees LinuxCNC stdout and stderr
  into the service journal and separate files; a nonzero session exit produces
  an automatic `/tmp/linuxcnc.report` containing the exact captured streams,
  command, exit status, and signal. If non-reaping `waitid` fails, the direct owner
  retains the child and degrades to nonblocking `wait4` until a real status is
  reaped instead of abandoning the process.
  It never restarts LinuxCNC, changes machine state, or sends a signal to a
  tracked process.
  See `docs/process-lifecycle-tracking.md` for the exact evidence boundary and
  the caught-signal limitations imposed by LinuxCNC 2.9.10.
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
- `python/dmc2_axis`: presentation-only AXIS integration required by AXIS. Its modules own
  notification handling, pendant-mode visibility, and the special AXIS entry
  point as separate responsibilities.
- `live`: the single accepted hardware profile and its NC programs.
- `tests/linuxcnc-motion`: the isolated real-LinuxCNC motion-consumer
  acceptance fixture.
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
   Bidirectional source checks reject both an unowned live userspace launch and
   a production direct-child catalog entry that is absent from the live profile.
   Every process-specific caught-signal mechanism is selected in that same
   catalog; absence of the initialization and handler-registration handshake
   remains explicit terminal evidence rather than being treated as proof that
   no signal occurred.
10. A LinuxCNC driver overlay must name an exact release commit and upstream
    commits, pass its recorded SHA-256 check, compile against the installed
    version, retain the installed module's export set, and deploy in the same
    rollback transaction as the custom realtime modules.

## Verification boundary

`scripts/verify.sh` formats and compiles the Rust workspace, then launches an
isolated LinuxCNC 2.9.10 instance with real `motmod`, the deployed serial and
task-monitor binaries, the deployed realtime controller, and a software step
generator. It sends production P3 packets through all X/Y/Z, x1/x10/x100, and
both-direction combinations and requires the selected LinuxCNC joint command
and downstream step count to move within the manual-jog tolerance while the
other axes remain still. The fixture sources the same pendant input and motion
contracts as the live profile. It also drives modeled raw and latched X/Y/Z
limit inputs through three complete production-controller stop and automatic
bounce paths while deliberately retaining the raw input. It requires an
operator-commanded x1 move on the attributed axis in the away direction,
verifies the native LinuxCNC hard-limit input remains masked throughout that
recovery, then clears the modeled raw input and requires latch reset and return
to ready. Finally, it passes the resulting real LinuxCNC error-channel records
through the production AXIS journal reader and rejects native joint-limit
errors.

That acceptance loads no HostMot2 or Mesa driver and therefore cannot prove
Ethernet delivery, physical step-pin output, drive response, motor movement,
physical limit wiring or switch response, or spindle behavior. Those boundaries
require a separately ordered live hardware observation. A successful build or
isolated acceptance must never be reported as proof of physical movement.
