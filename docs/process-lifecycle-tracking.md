# LinuxCNC process-lifecycle tracking

The evidence and limitations of the incident that required this tracker are
recorded in `docs/2026-09-02-linuxcnc-process-loss-investigation.md`.

## Problem boundary

LinuxCNC 2.9.10 starts the configured task, I/O process, HALUI process, and
HAL `loadusr` programs through `halcmd loadusr -Wn`. For example, it starts the
task with:

```text
halcmd loadusr -Wn inihal $EMCTASK -ini "$INIFILE" &
```

The pinned source in
`vendor/linuxcnc-2.9.10/src/hal/utils/halcmd_commands.cc` shows that `-Wn`
waits only until the named HAL component becomes ready. Once ready, `halcmd`
returns without retaining the later child wait status. A later death can
therefore leave stale HAL state while the kernel's exit code or terminating
signal is discarded. LinuxCNC also starts `linuxcncsvr` as a self-daemonizing
server and starts the persistent `rtapi_app` master through a hard-coded path.

## Implemented boundary

`config/processes.tsv` is the versioned source of truth for the role, exact
program, LinuxCNC launch site, ownership topology, criticality, backtrace
contract, INI-argument placement, core-dump policy, and kernel-visible owner
name of every tracked process.

The live INI and HAL configuration place `dmc2-process-supervisor` directly
around each configurable long-lived process whose later status LinuxCNC would
otherwise discard:

- `/usr/bin/milltask`;
- `/usr/bin/io`;
- `/usr/bin/halui`;
- `/usr/bin/axis`;
- `dmc2-serial-bridge`; and
- `dmc2-task-monitor`.

Each owner starts the unchanged configured program as its direct child. It
first uses `waitid(2)` with `WNOWAIT` for that exact PID, synchronizes a
terminal `/proc/<pid>` snapshot while the process remains a zombie, and only
then uses `wait4(2)` to reap it and obtain the authoritative status and
resource usage. The compiled launcher itself execs
`dmc2-session-supervisor`, which enables Linux `PR_SET_CHILD_SUBREAPER` before
starting the unchanged `/usr/bin/linuxcnc`. This makes later orphaned
`linuxcncsvr`, `rtapi_app`, and process-owner descendants waitable by a durable
session parent instead of PID 1. The session owner records every child it can
observe and every terminal status it reaps; unrecognised executables remain
explicitly uncatalogued rather than being guessed.

Each owner also sets its reviewed `comm` value before spawning the workload.
Unlike `/proc/<pid>/cmdline` and `/proc/<pid>/exe`, that value remains readable
after the owner becomes a zombie. The session tracker therefore can still map
a short-lived or already-dead owner to its exact role without relying on its
poll timing. If the subreaper first observes a new child in the fork-before-
`exec` interval, it continues classifying that PID and emits a
`session-child-reclassified` record when the catalogued owner identity appears;
the temporary shell identity is retained in the same record rather than
silently overwritten.

The shared journal records:

- tracker PID, child PID, logical role, launch site, ownership topology, and
  criticality;
- executable and argument bytes, hex encoded so tabs, newlines, and non-UTF-8
  bytes cannot corrupt the record;
- executable canonical path, inode metadata, selected safe environment values,
  wall-clock timestamps, and monotonic lifetime;
- both running and terminal-before-reap `/proc/<pid>` snapshots, including
  stat, status, command line, cgroup, limits, scheduler and scheduler counters,
  process I/O, signal masks, OOM scores, thread IDs, namespace, root,
  working-directory, and executable links;
- terminal cgroup membership, event, CPU, memory, PID-limit, I/O-pressure, and
  pressure-stall counters, with every unavailable controller recorded rather
  than silently omitted;
- directly decoded parent PID, process group, session ID, state, CPU ticks,
  thread count, and kernel start-time ticks;
- raw kernel wait status, exit code, terminating signal number and name, and
  core-dump status; and
- kernel `wait4` resource usage: user/system CPU, peak RSS, page faults, swaps,
  block I/O, IPC messages, delivered signals, and context switches.

The `waitid` and `wait4` interpretations are retained independently and the
terminal record states whether they agree. It also records every session child
known at the instant before reaping, so simultaneous `milltask`, `rtapi_app`,
or owner loss can be ordered from synchronized records rather than inferred
from whichever log line happened to be last.
Every session-descendant terminal record also names the LinuxCNC root PID and
includes a separately timestamped, non-reaping `waitid` probe of that exact
root. The recorded state distinguishes nonterminal, terminal-but-not-yet-
reaped, the root's own terminal event, already reaped, and probe failure. Thus
a `milltask` owner departure during a root observed nonterminal is represented
directly rather than inferred from a later stale heartbeat.

For `milltask`, a matching LinuxCNC-generated `/tmp/backtrace.<pid>` is copied
to the durable log directory before the terminal event is committed. The same
capture is performed by the session subreaper if an actual `milltask` workload
is orphaned because its direct owner failed first.
The process catalog also raises each tracked child's soft `RLIMIT_CORE` to its
inherited hard limit before `exec`. The inherited and requested values plus the
host's `core_pattern`, `core_uses_pid`, and `suid_dumpable` settings are
recorded. When the retained `waitid` status reports a core-generating death,
the owner resolves this host's file-based core policy, rejects a stale or
non-regular source, opens it without following a symbolic link, verifies its
device/inode identity and size, and synchronizes a PID/timestamp-named copy in
the lifecycle-journal directory before `wait4` reaps the child. Keeping the
child waitable during the potentially large copy means an outer subreaper can
still recover its status if the direct owner itself fails. The terminal record
contains the source and copy paths, source identity, size, selected working-
directory evidence, and the exact failure state if no copy can be retained.
Pipe handlers and
percent-expanded core templates are identified explicitly rather than guessed.
This does not claim that a core exists when the wait status or host policy says
otherwise. In particular, the setuid `rtapi_app` remains subject to the host's
setuid core-dump policy.

Each journal line is append-only, terminated by a newline, protected with a
CRC-32, serialized against all concurrent process owners with an advisory file
lock, and synchronized before execution continues. A partial final record from
an interrupted write is separated from the next session and reported in the
following tracker-start record.

The compiled tests exercise real OS children, a real retained zombie snapshot
followed by reap, independently agreeing `waitid`/`wait4` records, real
non-zero exits, real signal termination, inherited core-limit application,
concurrent writers, Linux subreaper adoption, and zombie-safe owner identity.
They also exercise an adopted descendant terminating while the session root
continues to run and the LinuxCNC caught-fatal-signal behavior in which a
matching SIGSEGV backtrace must override an apparent zero exit.
Separate real SIGSEGV cases require a nonempty kernel core to be retained from
both a direct child and an adopted session descendant. Static launch-coverage
checks are bidirectional: every active live INI/HAL userspace launch must have
a catalogued owner, every production direct-child catalog row must occur in the
live profile, and the two hard-coded persistent processes must remain present
in the pinned LinuxCNC source.
An additional real-process case kills the direct owner with `SIGKILL`, then
requires the outer subreaper to retain that exact owner status and the adopted
workload's later `SIGSEGV` status and nonempty core.
The complete lifecycle integration set is repeatedly run to cover the
fork/`exec` classification race as well as the terminal path.
Those tests establish the tracker mechanics. They are not a claim
that the currently running, older LinuxCNC session has this new ownership
topology; it takes effect on the next explicitly requested launch.

## Deliberate non-behavior

Neither tracker restarts, stops, signals, E-stops, enables, disables, homes,
jogs, commands a spindle, or performs recovery. A child failure is retained as
evidence, never silently relabelled as harmless, but the tracker does not
invent an automatic machine-state response.

LinuxCNC 2.9.10 catches `SIGINT` and `SIGTERM`, sets its own shutdown flag, and
eventually calls `exit(0)`. Its `SIGSEGV` and `SIGFPE` handler also writes a
backtrace, sets the shutdown flag, and eventually calls `exit(0)`. Consequently
the parent wait status cannot identify those caught signals. The direct owner
preserves the latter two from LinuxCNC's matching backtrace header; it does not
guess whether a zero exit was initiated by caught `SIGINT`, caught `SIGTERM`,
or an ordinary return. If the operating system terminates both a direct owner
and child together, no code inside the terminated owner can guarantee its own
final record; its already-synchronized start record remains, and the outer
session owner can record the direct owner's kernel termination while it stays
alive.

This instrumentation cannot recover the exact wait status of the `milltask`
that died before it was installed. The final nearby Modbus address-mismatch
message is correlation, not proof of why the process ended. Exact lifecycle
attribution begins with a subsequently launched tracked session. It also
cannot identify who sent a caught `SIGINT`/`SIGTERM`, because LinuxCNC converts
those signals into a later zero exit before the parent can receive a terminal
wait status; resolving that narrower provenance boundary would require
separate kernel signal-audit instrumentation and is not claimed here.
