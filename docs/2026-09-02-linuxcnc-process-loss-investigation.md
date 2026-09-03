# LinuxCNC process-loss investigation — 2026-09-02

## What was observed

The affected live session retained its outer LinuxCNC shell, AXIS,
`linuxcncsvr`, I/O, HALUI, the serial bridge, and the task monitor. The two
processes that owned the controller itself were absent:

- `/usr/bin/milltask`, historical PID 37318; and
- `/usr/bin/rtapi_app`, historical PID 37265.

The user systemd unit still reported `active (running)` because its main shell
PID remained alive. That status proved only that the shell existed; it did not
prove that the LinuxCNC task or realtime controller existed. A later
`halcmd show comp` had no persistent controller components.

The command and journal timeline was:

- 17:11:49 local: the last audited action before the loss was a read-only HAL
  pin query;
- 17:11:55.318358: PID 37318 (`milltask`) emitted
  `hm2_modbus.0: error: Modbus device address mismatch: got 0x00, expected 0x01`;
- 17:11:55.319877: PID 37265 (`rtapi_app`) emitted the same line, about 1.5 ms
  later; and
- 17:11:56: the next read-only HAL query could no longer obtain controller
  state.

The audited command stream contains no stop, unload, restart, signal, or kill
operation around the event. The kernel journal contains no matching OOM,
segfault, trap, core, or explicit signal report. There was no
`/tmp/backtrace.37318`, and the service descendants inherited a zero soft core
limit. The service cgroup did not hit its PID limit, and realtime CPU time was
unlimited.

## What the evidence does and does not establish

The address-mismatch message is tightly correlated with the loss of both
processes. It is not proof that the mismatch terminated either process. In the
pinned LinuxCNC 2.9.10 source, that message's local driver path reports the
bad response and requests a resend; it does not intentionally exit
`milltask` or `rtapi_app`.

LinuxCNC started the task through `halcmd loadusr -Wn`. Once the component was
ready, that `halcmd` process exited and discarded responsibility for the
task's eventual wait status. Therefore the kernel's historical exit code,
terminating signal, and core flag were already irretrievably reaped before
this investigation. The exact historical cause cannot honestly be recovered
from the remaining logs.

Upstream LinuxCNC issue 3849 and its 2.9 backport address a separate
`hm2_modbus` inter-character-delay lock-up. The pinned 2.9.10 tree already
contains that backport, and its described lock-up is not evidence of these two
process deaths.

## Tracking correction

Future launches use two independent ownership layers:

1. `dmc2-process-supervisor` directly owns each configurable long-lived
   process, including `milltask`, and cannot lose its eventual kernel status to
   `halcmd -Wn`.
2. `dmc2-session-supervisor` is a Linux child subreaper for orphaned
   `rtapi_app`, `linuxcncsvr`, and process-owner descendants.

The corrected terminal path is deliberately two-stage:

1. `waitid(WEXITED | WNOWAIT)` reports termination without reaping;
2. the tracker synchronizes the child's still-readable zombie `/proc` state,
   complete tracked-child membership, and cgroup resource/pressure counters;
3. any matching LinuxCNC task backtrace is preserved;
4. `wait4` then captures the authoritative raw status and complete resource
   usage; and
5. the journal records whether the independent `waitid` and `wait4`
   interpretations agree.

Each process owner has a unique, reviewed kernel `comm` value so its role stays
identifiable even if it dies before the session poller reads its command line.
Tracked children receive the data-selected soft core limit before `exec`, and
the journal records both limits and the host core-dump policy.

All records are appended to
`var/log/linuxcnc/process-lifecycle.tsv` under an exclusive lock, include a
CRC-32, and are synchronized before reaping proceeds.

## Deliberate non-behavior and remaining proof boundary

The tracking layer does not restart, stop, signal, E-stop, home, jog, or
otherwise recover or control the machine. A surviving shell is no longer the
only available evidence of session health, but no automatic machine response
has been invented.

Offline tests directly prove retained-zombie capture, real exit and signal
status, `waitid`/`wait4` agreement, core-limit inheritance, concurrent journal
writes, subreaper adoption, and owner identity after death. They do not prove
that the currently stopped/older live session used this new binary, nor do
they establish the cause of the historical event. The live evidence boundary
begins only after a separately authorized launch of the newly built tracker.

LinuxCNC catches `SIGINT` and `SIGTERM` and later returns zero, so ordinary
parent wait status cannot identify the sender of either caught signal. Exact
sender provenance for that narrow case would require separately authorized
kernel signal auditing; it is not claimed by this implementation.

## Primary source references

- LinuxCNC issue 3849: <https://github.com/linuxcnc/linuxcnc/issues/3849>
- Upstream correction: <https://github.com/LinuxCNC/linuxcnc/commit/582ac390274aac5543b18f66dacd271564b5ee27>
- 2.9 backport: <https://github.com/LinuxCNC/linuxcnc/commit/ccb56bf04771713e800fde636de94a275d8143c2>
