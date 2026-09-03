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
contract, INI-argument placement, core-dump policy, kernel-visible owner name,
rolling live-snapshot period, and caught-signal evidence mechanism of every
tracked process. Production roles
currently use a one-second period; verification roles use ten milliseconds so
the real refresh path can be exercised without slowing the suite.

The live INI and HAL configuration place `dmc2-process-supervisor` directly
around each configurable long-lived process whose later status LinuxCNC would
otherwise discard:

- `/usr/bin/milltask`;
- `/usr/bin/io`;
- `/usr/bin/halui`;
- `/usr/bin/axis`;
- `dmc2-serial-bridge`; and
- `dmc2-task-monitor`.

Each owner starts the unchanged configured program as its direct child. While
that process is alive, the owner polls `waitid(2)` without reaping and retains
the newest successful `/proc/<pid>` resource and open-file snapshot in memory.
On termination it uses `waitid(2)` with `WNOWAIT` for that exact PID,
synchronizes both the retained last-live snapshot and a terminal
`/proc/<pid>` snapshot while the process remains a zombie, and only then uses
`wait4(2)` to reap it and obtain the authoritative status and resource usage.
If the non-reaping `waitid` syscall fails, the direct owner no longer exits and
drops a potentially live child. It records the failure, keeps taking rolling
snapshots, and switches that same PID to nonblocking `wait4(WNOHANG)` polling.
That fallback retains the real terminal status and resource usage, then returns
a tracking-failure status only after the child has actually been reaped. It
explicitly marks the zombie `/proc` snapshot as unavailable because `wait4`
must reap to report the fallback result. Repeated fallback-poll errors keep the
owner alive, are counted, and produce transition/recovery records instead of
silently abandoning the child or flooding the journal.
The session subreaper applies the same rule to `waitid(P_ALL)`: after a failure
it retains the session and uses nonblocking `wait4(-1)` until every real child
status has been reaped, then returns tracking failure rather than disguising
the degraded evidence path as a normal LinuxCNC exit.
If `waitid(WNOWAIT)` succeeds but the following exact-PID `wait4` temporarily
fails, neither owner discards the retained status. It keeps the child waitable,
continues its applicable signal drain, child discovery, and rolling snapshots,
records error transitions and exact attempt/failure/recovery counts, and retries
the same exact-PID reap until the kernel status and resource usage are obtained.
Any such degradation makes the owner return tracking failure after the status
is safely recorded; it is never relabelled as an ordinary child exit.
For the three pinned LinuxCNC C programs that install SIGINT and SIGTERM with
libc `signal()`—`milltask`, `io`, and `halui`—the catalog also selects
`libc-signal-int-term-v1`. Their direct owner supplies one side of a dedicated
nonblocking Unix sequenced-packet socket and loads
`libdmc2_signal_evidence.so` for that one `exec`.
The library removes its preload variables before the program runs and marks
the evidence descriptor close-on-exec, so it does not propagate through later
program executions. It wraps only those two signal registrations; every other
`signal()` call is delegated to libc's original implementation.

The fixed-size packet is sent with `MSG_NOSIGNAL` before the program's original
handler is called. Thus a dead owner or closed reader cannot raise SIGPIPE in
the observed process. The send is deliberately nonblocking so evidence
collection cannot stall the observed process. It is therefore best-effort: the
socket has a recorded finite buffer, there is no signal-safe drop counter, and
absence of a delivery packet is evidence absence—not proof that no signal was
delivered. The packet retains the signal number, `si_code`,
`si_errno`, receiving PID and TID, kernel-supplied sender PID and UID, and
realtime timestamp. Initialization and each handler arm/disarm are separate
records. The owner validates the library's regular-file permissions and ELF
ABI before spawn, proves the exact socket endpoints with a real nonblocking
send/receive loopback, records socket send-buffer capacity (or the structured
failure of that optional metadata query) and library identity,
continuously drains the channel, and drains it again after `waitid(WNOWAIT)`
reports the child terminal but before reaping. Missing initialization,
incomplete registration, foreign target records, invalid records, malformed
packets, read failure, and channel closure are distinct states.
An intact caught SIGINT/SIGTERM followed by exit zero is classified separately
from an intact zero exit for which no caught-signal record exists.
The compiled launcher itself execs
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
  systemd invocation/journal identifiers, wall-clock timestamps, and monotonic
  lifetime;
- the last successful live `/proc/<pid>` snapshot, including stat, status,
  command line, cgroup, limits, scheduler and scheduler counters, process I/O,
  memory-map identity, wait channel, current syscall, OOM scores, and thread
  IDs;
- the live open-file count, retained descriptor targets, and matching `fdinfo`,
  including explicit read-race failures and truncation counters;
- the snapshot sequence, capture duration, age at process death, number of
  attempts and successes, and exact result of the final refresh attempt;
- the first transition into a live-snapshot read failure and the later
  restoration as synchronized journal events, without repeating the same
  failure every polling cycle;
- the terminal-before-reap `/proc/<pid>` snapshot, including namespace, root,
  working-directory, and executable links that remain available;
- terminal cgroup membership, event, CPU, memory, PID-limit, I/O-pressure, and
  pressure-stall counters, with every unavailable controller recorded rather
  than silently omitted;
- directly decoded parent PID, process group, session ID, state, CPU ticks,
  thread count, and kernel start-time ticks;
- raw kernel wait status, exit code, terminating signal number and name, and
  core-dump status; and
- caught-signal initialization and registration state, every retained delivered
  SIGINT/SIGTERM `siginfo_t`, kernel-defined sender-identity validity, and the
  last matching delivery summarized on the terminal record;
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

Each completed journal event is append-only, terminated by a newline, protected
with a CRC-32, serialized against all concurrent process owners with an
advisory file lock, and synchronized before execution continues. Both journal open and every
append inspect the final record while holding that lock. If a process was
killed during a write, the next surviving writer separates the partial bytes
before appending anything else and emits a checksummed recovery record with the
fragment's byte offset, length, and CRC-32. The fragment remains in the journal
as evidence but cannot merge with or masquerade as the next valid event. A
partial record recovered while opening the journal is also reported by the
following tracker-start record.
After spawn, journal failures are retained in bounded memory: the first full
error, the last structured error, exact failure count, recovery count, and
whether failure is still active. A persistent storage failure therefore cannot
grow an unbounded error vector inside the process owner. The terminal event
includes that summary when persistence has recovered; each failed append also
renders its complete fallback event and error to standard error.

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
That case also validates any journal fragment left when `SIGKILL` interrupts a
large record: the following recovery record must identify the fragment by
offset, byte count, and checksum before later lifecycle records are accepted.
Two real-process cases open a uniquely named file only after the initial
snapshot. They require the terminal-before-reap record to contain that exact
descriptor from a later rolling snapshot, once through a direct owner and once
through an adopted session descendant. Those cases exercise refresh and
retention rather than manufacturing a downstream result.
The complete lifecycle integration set is repeatedly run to cover the
fork/`exec` classification race as well as the terminal path.
Two additional real-process cases compile the exact production interposer and
a C child that matches LinuxCNC's `signal()` registration and in-handler rearm
behavior. Independent `/bin/kill` processes deliver real SIGINT and SIGTERM.
The tests require the kernel-provided sender PID/UID, target PID, registration
handshake, channel close, zero child exit, and caught-signal outcome to agree;
they do not inject the expected terminal state into the tracker.
A separate owner-loss case kills the Rust reader first, then delivers SIGTERM
to the now-adopted C workload. The session subreaper must observe exit zero,
not SIGPIPE; this exercises the `MSG_NOSIGNAL` failure path with real processes.
Two real-process cases interpose the direct owner's or session subreaper's
first `waitid` call and three subsequent `wait4` calls, forcing `EPERM` while a
real shell child remains live. The tests require each owner to remain present,
count the repeated fallback failures without journal flooding, record recovery,
retain the child's own exit through nonblocking `wait4`, reap that PID, write
the degraded terminal record, and return tracking failure. They do not inject
a child exit status.
Two more real-process cases allow `waitid(WNOWAIT)` to retain an actual child
exit and then hold exact-PID `wait4` in a repeated `EPERM` failure state. Each
test requires the owner and the zombie child to remain present, releases the
fault, and then requires the independently agreeing real wait status, recovery
record, exact failure count, and tracking-failure return. The session case also
continues child discovery while the retained reap is unavailable.
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
the parent wait status alone cannot identify those caught signals. The direct
owner preserves SIGSEGV/SIGFPE from LinuxCNC's matching backtrace header and
preserves caught SIGINT/SIGTERM from the pre-handler `siginfo_t` channel. A
zero exit without either record stays explicitly unattributed rather than
being guessed. If the operating system terminates both a direct owner
and child together, no code inside the terminated owner can guarantee its own
final record; its already-synchronized start record remains, and the outer
session owner can record the direct owner's kernel termination while it stays
alive.

This instrumentation cannot recover the exact wait status of the `milltask`
that died before it was installed. The final nearby Modbus address-mismatch
message is correlation, not proof of why the process ended. Exact lifecycle
attribution begins with a subsequently launched tracked session. For
process-directed signals whose Linux `si_code` defines sender identity, the
recorded PID/UID is kernel evidence. It is not an executable-name guarantee:
a short-lived sender can exit before userspace can read its `/proc` identity,
and exact historical sender ancestry would require kernel audit/trace data.
Kernel-generated signals explicitly mark sender identity invalid.
Because the pre-handler transport is nonblocking and finite, a missing caught-
signal packet cannot be promoted into a claim that no signal occurred. The
terminal outcome deliberately says `without-caught-signal-evidence`, and the
journal identifies the transport semantics as `best-effort-nonblocking`.

The interposer is intentionally not applied to set-user-ID `rtapi_app` (the
dynamic loader ignores `LD_PRELOAD` in secure-execution mode), the shell
session root, or AXIS. Their uncaught terminal status and cores remain covered
by the direct/session ownership layers, but a caught signal that those
processes themselves convert to a clean exit does not gain sender provenance
from this mechanism. If the direct owner itself is killed before it can drain
and synchronize a final signal record, the outer subreaper still retains the
owner and adopted-workload terminal statuses, but it cannot reconstruct packets
that were never drained from the dead owner's private socket.
The retained resource snapshot can be up to its catalogued period old. It is
evidence of the last successfully observed live state, not a claim that every
resource mutation in the final second was observed. Individual large fields
and descriptor catalogs are bounded; every omission is reported with explicit
entry, error-entry, and payload truncation counters rather than silently
presented as complete.
