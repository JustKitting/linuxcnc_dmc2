# `milltask` lifecycle tracking

## Problem boundary

LinuxCNC 2.9.10 starts its configured task with:

```text
halcmd loadusr -Wn inihal $EMCTASK -ini "$INIFILE" &
```

The pinned source in
`vendor/linuxcnc-2.9.10/src/hal/utils/halcmd_commands.cc` shows that `-Wn`
waits only until the `inihal` component becomes ready. Once ready, `halcmd`
returns without retaining the child wait status. A later `milltask` death can
therefore leave a stale HAL component record while the kernel's exit code or
terminating signal is discarded.

## Implemented boundary

The live `[TASK]` command starts `dmc2-milltask-supervisor`, which starts the
unchanged `/usr/bin/milltask` as its direct child and remains blocked in
`wait(2)` for that exact PID. It records:

- supervisor PID and exact `milltask` PID;
- executable and argument bytes, hex encoded so tabs, newlines, and non-UTF-8
  bytes cannot corrupt the record;
- wall-clock timestamps and monotonic lifetime;
- raw kernel wait status, exit code, terminating signal number and name, and
  core-dump status;
- matching LinuxCNC-generated `/tmp/backtrace.<pid>` evidence, copied to the
  durable log directory before the terminal event is committed.

Each journal line is append-only, terminated by a newline, protected with a
CRC-32, and synchronized before execution continues. A partial final record
from an interrupted write is separated from the next session and reported in
the following supervisor-start record.

## Deliberate non-behavior

This component does not restart, stop, signal, E-stop, enable, disable, home,
jog, command a spindle, or perform recovery. It only launches the configured
task, waits for that child, and records what the operating system returns.

LinuxCNC 2.9.10 catches `SIGINT` and `SIGTERM`, sets its own shutdown flag, and
eventually calls `exit(0)`. Its `SIGSEGV` and `SIGFPE` handler also writes a
backtrace, sets the shutdown flag, and eventually calls `exit(0)`. Consequently
the parent wait status cannot identify those caught signals. The supervisor
preserves the latter two from LinuxCNC's matching backtrace header; it does not
guess whether a zero exit was initiated by caught `SIGINT`, caught `SIGTERM`,
or an ordinary return. If the operating system terminates both supervisor and
child together, no code inside the terminated supervisor can guarantee a final
record; its already-synchronized start record remains.

This instrumentation cannot recover the cause of a process that died before
it was installed. It establishes evidence for subsequent launches only.
