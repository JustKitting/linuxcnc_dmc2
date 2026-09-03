# LinuxCNC process-loss investigation — 2026-09-02

## What was observed

The affected live session eventually retained its outer LinuxCNC shell, AXIS,
`linuxcncsvr`, I/O, HALUI, the serial bridge, and the task monitor while both
controller processes were absent:

- `/usr/bin/milltask`, historical PID 37318; and
- `/usr/bin/rtapi_app`, historical PID 37265.

The user systemd unit still reported `active (running)` because its main shell
PID remained alive. That status proved only that the shell existed; it did not
prove that the LinuxCNC task or realtime controller existed. A later
`halcmd show comp` had no persistent controller components.

They did **not** disappear together. The command and journal timeline was:

- 17:11:49 local: the last audited action before the loss was a read-only HAL
  pin query;
- 17:11:55.318358: PID 37318 (`milltask`) emitted
  `hm2_modbus.0: error: Modbus device address mismatch: got 0x00, expected 0x01`;
- 17:11:55.319877: PID 37265 (`rtapi_app`) emitted the same line, about 1.5 ms
  later;
- no later journal entry exists for PID 37318, so this is the last surviving
  evidence from `milltask`;
- PID 37265 continued emitting messages for another 73 minutes, proving that
  `rtapi_app` remained alive after `milltask` disappeared;
- 18:24:57.774835: PID 37265 reported an unexpected realtime delay on the
  1 ms task;
- 18:25:18.475467: PID 37265 reported
  `hm2/hm2_7i95.0: error finishing read! iter=7085925`; and
- 18:25:23.801163: PID 37265 reported
  `rtapi_app: caught signal 11 - dumping core`.

The audited command stream contains no stop, unload, restart, signal, or kill
operation around the `milltask` disappearance. There was no
`/tmp/backtrace.37318`. The service descendants inherited a zero soft core
limit, the service cgroup did not hit its PID limit, and realtime CPU time was
unlimited. No core or backtrace artifact from PID 37265 remains, despite its
explicit SIGSEGV report. The surviving evidence does not establish why that
artifact is absent.

## What the evidence does and does not establish

The address-mismatch message is tightly correlated with the last surviving
`milltask` output. It is not proof that the mismatch terminated `milltask`.
The later messages from PID 37265 prove that this mismatch did not terminate
`rtapi_app` at 17:11:55. In the pinned LinuxCNC 2.9.10 source, that message's
local driver path reports the bad response and requests a resend; it does not
intentionally exit either process.

The later `rtapi_app` event is different: its own log explicitly identifies
SIGSEGV. The nearby realtime-delay and HostMot2 read-error messages establish
ordering and context, not causation. Without the terminal wait record, core,
or stack trace, the exact code path that generated the SIGSEGV is not
recoverable from this historical session.

LinuxCNC started the task through `halcmd loadusr -Wn`. Once the component was
ready, that `halcmd` process exited and discarded responsibility for the
task's eventual wait status. The same ownership gap applied to the persistent
`rtapi_app` process. Therefore their kernel exit codes, terminating signals,
core flags, and resource usage had already been irretrievably reaped before
this investigation. The exact historical cause of `milltask`'s disappearance
cannot honestly be recovered from the remaining logs; only `rtapi_app`'s
self-reported SIGSEGV is known.

Upstream LinuxCNC issue 3849 and its 2.9 backport address a separate
`hm2_modbus` inter-character-delay lock-up. The pinned 2.9.10 tree already
contains that backport, and its described lock-up is not evidence of these two
process deaths.

The audit did identify a different, concrete defect in the exact installed
2.9.10 `hm2_eth` source. A queued-write `send()` failure returns before
resetting `write_packet_ptr` and `write_packet_size`; later servo cycles keep
appending to the fixed 1400-byte array with no bounds check. Upstream commit
`10dc650adf741da16f16ffe5a850e78b91aa3ec7` describes this old path as
generating a segfault and adds read/write queue bounds checks. Its companion
commit `cd8eb00ad7d31f81fd40e674987c6edbc3bed23c` resets the write queue even
when `send()` fails. Both `hm2_eth` changes apply cleanly to the exact 2.9.10
tag and retain the module's exported-symbol set.

The historical journal has no retained `ERROR: sending packet` line, so this
known defect cannot honestly be claimed as the proven cause of PID 37265's
old SIGSEGV. It is nevertheless a reachable memory-corruption path in the
installed driver and is removed by the source-controlled overlay in
`patches/linuxcnc-2.9.10/hm2-eth-buffer-safety.patch`. The overlay catalog and
builder verify the 2.9.10 base commit, patch checksum, upstream provenance,
installed version, and ABI exports. Building stages an artifact only; live
deployment and hardware behavior remain unverified until a separately ordered
launch installs and loads it.

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

Session terminal records include the LinuxCNC root PID and a separately
timestamped, non-reaping `waitid` probe of that exact root. This distinguishes
an observed-nonterminal root from one already terminal or reaped when a
controller-critical owner departs, while retaining probe failures explicitly.

Each process owner has a unique, reviewed kernel `comm` value so its role stays
identifiable even if it dies before the session poller reads its command line.
The session tracker also upgrades and journals an identity first seen in the
fork-before-`exec` window, retaining both the temporary identity and final
catalogued owner identity.
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
writes, subreaper adoption, owner identity after death, descendant departure
while the session root stays live, and preservation of a matching LinuxCNC
SIGSEGV backtrace despite the child's later zero exit. They do not prove
that the currently running older session used this new binary, nor do
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
- HostMot2 Ethernet bounds fix: <https://github.com/LinuxCNC/linuxcnc/commit/10dc650adf741da16f16ffe5a850e78b91aa3ec7>
- HostMot2 failed-send queue reset: <https://github.com/LinuxCNC/linuxcnc/commit/cd8eb00ad7d31f81fd40e674987c6edbc3bed23c>
