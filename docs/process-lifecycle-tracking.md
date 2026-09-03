# LinuxCNC process-lifecycle tracking

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
contract, and INI-argument placement of every tracked process.

The live INI and HAL configuration place `dmc2-process-supervisor` directly
around each configurable long-lived process whose later status LinuxCNC would
otherwise discard:

- `/usr/bin/milltask`;
- `/usr/bin/io`;
- `/usr/bin/halui`;
- `/usr/bin/axis`;
- `dmc2-serial-bridge`; and
- `dmc2-task-monitor`.

Each owner starts the unchanged configured program as its direct child and
uses `wait4(2)` for that exact PID. The compiled launcher itself execs
`dmc2-session-supervisor`, which enables Linux `PR_SET_CHILD_SUBREAPER` before
starting the unchanged `/usr/bin/linuxcnc`. This makes later orphaned
`linuxcncsvr`, `rtapi_app`, and process-owner descendants waitable by a durable
session parent instead of PID 1. The session owner records every child it can
observe and every terminal status it reaps; unrecognised executables remain
explicitly uncatalogued rather than being guessed.

The shared journal records:

- tracker PID, child PID, logical role, launch site, ownership topology, and
  criticality;
- executable and argument bytes, hex encoded so tabs, newlines, and non-UTF-8
  bytes cannot corrupt the record;
- executable canonical path, inode metadata, selected safe environment values,
  wall-clock timestamps, and monotonic lifetime;
- raw `/proc/<pid>` stat, status, command line, cgroup, limits, scheduler, OOM,
  namespace, root, working-directory, and executable-link snapshots;
- directly decoded parent PID, process group, session ID, state, CPU ticks,
  thread count, and kernel start-time ticks;
- raw kernel wait status, exit code, terminating signal number and name, and
  core-dump status; and
- kernel `wait4` resource usage: user/system CPU, peak RSS, page faults, swaps,
  block I/O, IPC messages, delivered signals, and context switches.

For `milltask`, a matching LinuxCNC-generated `/tmp/backtrace.<pid>` is copied
to the durable log directory before the terminal event is committed.

Each journal line is append-only, terminated by a newline, protected with a
CRC-32, serialized against all concurrent process owners with an advisory file
lock, and synchronized before execution continues. A partial final record from
an interrupted write is separated from the next session and reported in the
following tracker-start record.

The compiled tests exercise real OS children, real non-zero exits, real signal
termination, concurrent writers, and Linux subreaper adoption of an orphaned
descendant. Those tests establish the tracker mechanics. They are not a claim
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
attribution begins with a subsequently launched tracked session.
