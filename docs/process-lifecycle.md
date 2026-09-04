# LinuxCNC process ownership and lifecycle evidence

The historical process-loss incident is recorded in
`docs/2026-09-02-linuxcnc-process-loss-investigation.md`. This document states
only what the current implementation owns and records.

## Why an owner is needed

LinuxCNC 2.9.10 starts several configured programs through
`halcmd loadusr -Wn`. That command waits for a HAL component to become ready,
then returns. It does not remain the long-lived process's parent and cannot
later retain that process's kernel wait status.

## Data-driven ownership

`config/processes.tsv` is the compiled source of truth for each production
role's exact program, LinuxCNC launch site, ownership topology, criticality,
argument placement, core-limit policy, optional backtrace contract, and
kernel-visible owner name.

The live INI and HAL place `dmc2-process-supervisor` directly around these
configurable long-lived processes:

- `milltask`;
- I/O;
- HALUI;
- AXIS;
- the serial bridge; and
- the task monitor.

The compiled launcher places `/usr/bin/linuxcnc` directly beneath
`dmc2-session-supervisor`. The session supervisor enables Linux
`PR_SET_CHILD_SUBREAPER`, so orphaned descendants remain waitable by that
session owner rather than being reparented directly to PID 1.

## Direct-child supervisor

For a catalogued direct-child role, the supervisor:

1. validates the role, exact executable, and argument placement;
2. sets and reads back the catalogued kernel process name;
3. applies the catalogued child core limit;
4. starts exactly one child and retains its `Child` ownership;
5. records the start identity and exact argument bytes;
6. waits for that exact PID with `wait4`; and
7. records the raw status, exit code or terminating signal, resource usage,
   elapsed lifetime, and any matching LinuxCNC task backtrace.

A transient `wait4` failure does not release the child. The supervisor records
the concrete error and recovery action, retains ownership, and retries the same
PID. Recovery is recorded when `wait4` succeeds. If the PID is already absent
and no status can be recovered, the supervisor returns an explicit
`WaitStatusLost` error rather than inventing a terminal result.

## Session supervisor

The session owner starts the unchanged `/usr/bin/linuxcnc`, retains the root
status, and reaps adopted descendants with nonblocking `wait4`. It records
known child identities when they become its direct children and records every
terminal status it obtains. Child-scan and wait failures have explicit active
and recovered transitions; neither failure causes an automatic process signal,
machine command, or restart.

LinuxCNC stdout and stderr are forwarded to the service journal and copied to
separate files under `var/log/linuxcnc/`. After a failed session, the captured
streams, exact command, and terminal status are written atomically to
`/tmp/linuxcnc.report`.

## Journal contract

`var/log/linuxcnc/process-lifecycle.tsv` is shared by all owners. Records are
serialized with an advisory file lock, end with CRC-32, and are synchronized.
Before each append, an interrupted final record is separated so it cannot be
joined to a later valid record. After spawn, a journal failure is reported to
standard error, retained in bounded state, retried on later events, and
reported as recovered only after a successful append.

## Deliberate non-behavior

The ownership layer does not restart LinuxCNC, kill a process, send a signal,
E-stop, enable, disable, home, jog, command a spindle, clear a machine fault,
or choose an operator mode. It records process lifecycle evidence only.

## Evidence limits

- A successful build proves compilation only.
- A running supervisor proves only that the supervisor process exists.
- A direct-child wait record proves the kernel status returned for that child;
  it does not prove why the child exited.
- LinuxCNC programs may catch a signal and later exit zero. `wait4` cannot
  recover the caught signal in that case, so zero exit must not be described as
  proof that no signal was delivered.
- The session supervisor observes descendants only when they are its direct
  children. A short-lived adopted child can terminate before its executable is
  identified; its status remains real, but its role can remain uncatalogued.
- A matching `/tmp/backtrace.<pid>` is additional evidence supplied by
  LinuxCNC's fatal-signal handler. Absence of that file is not proof that no
  fault occurred.
- Lifecycle ownership does not prove HAL routing, LinuxCNC motion-consumer
  acceptance, physical motion, recovery behavior, or UI survival.
- No current live session is shown to use a new build until an explicitly
  authorized launch is inspected at each required boundary.
