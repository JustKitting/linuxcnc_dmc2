# Standalone rs274 truncated the live tool-data mapping

## Retained evidence

- Installed package: `linuxcnc-uspace` `1:2.9.10`.
- Source base: `86cdca76fa2a36274c432caa21952b23c267989a`.
- Core: `live/core.527197`, from `/usr/bin/milltask -ini
  /home/kit/linuxcnc_dmc2/live/dmc2.ini`, timestamp
  `2026-09-05 09:39:11.779093772 -0400`.
- The lifecycle journal records `milltask` PID 527197 receiving `SIGBUS`
  (signal 7), with a core dump and no LinuxCNC task backtrace file.
- Core signal information: `si_code=2` (`BUS_ADRERR`),
  `si_addr=0x7f838a7000`. That is the start of the core's mapping of
  `/home/kit/.tool.mmap` (`0x7f838a7000..0x7f838c3000`).
- Stack: `libtooldata` mutex access → `tooldata_get()` →
  `EMC_TOOL_STAT::operator=()` → `Task::emcIoUpdate()` → `main()`.
- The assistant's standalone parser output and temporary parameter file
  were written at `09:39:11.803093928 -0400`, approximately 24 ms later.

## Cause

At the pinned source base, `src/emc/sai/driver.cc:570` calls
`tool_mmap_creator(NULL, ...)` before argument parsing. In
`src/emc/tooldata/tooldata_mmap.cc`, that function opens the fixed
`$HOME/.tool.mmap` with `O_RDWR | O_CREAT | O_TRUNC`, then expands and maps it.
The I/O controller, task, HALUI and AXIS use the same file and inode.

Truncation temporarily reduces the backing file to zero bytes. A controller
access in that interval faults with `SIGBUS`; otherwise it can silently see
the parser's replacement tool data. The source path and retained fault
address identify the failure mechanism. An exact syscall trace of the
original invocation was not captured.

The INI, `-t` tool-table selection, `-v` parameter-file selection, and current
directory do not isolate this mapping. Even `rs274 --help` reaches its
creation first. SysV IPC separation alone also cannot isolate a shared
filesystem pathname. The typed `dmc2ctl inspect-file` path does not invoke
`rs274`; the assistant invoked the vulnerable standalone binary separately.

Upstream commit
[`8c8e0484707794c3d0706679653d108e049a505c`](https://github.com/LinuxCNC/linuxcnc/commit/8c8e0484707794c3d0706679653d108e049a505c)
addresses this collision with a private temporary pathname for `rs274` and
ownership-aware library cleanup. The local checkout of that upstream commit
was inspected, including its diff and parent history.

## Local correction

The 2.9.10 overlay enforces the already-existing NULL-status standalone
contract inside `tool_mmap_creator()`. It allocates `MAP_PRIVATE |
MAP_ANONYMOUS` storage and returns without resolving or opening the shared
tool-file path. This covers the installed, unmodified `/usr/bin/rs274` with
the same exported library ABI. It does not require a new launcher or changing
`HOME`, and it does not create a temporary file to reopen by name.

The internal mapping type distinguishes Unmapped, Standalone, SharedCreator
and SharedUser. Standalone cleanup only unmaps its memory. Only the shared
creator may unlink the controller file; shared users close their own retained
descriptor. Cleanup resets its state so a second close is a no-op. Allocation
failure exits the standalone process with the OS cause and a retry action,
without falling back to the controller's file.

The source is applied as a hash-pinned patch to an archive of the pristine
vendor checkout. The build compiles the three upstream tooldata translation
units against the installed 2.9.10 development headers and compares the
exported-symbol contract. The standard Rust launcher includes the staged
library in its system-artifact byte checks and existing system install
path, so a package replacement is detected on the next standard launch.

The existing installer also accepts `--userspace-only`, selecting only typed
userspace libraries. It stages and checks the new release, then atomically
renames each artifact into place, preserving already-loaded inodes. It creates
no backup copies of installed artifacts; source history remains in Git. A
partial installation leaves the already-replaced artifacts in place, reports
the cause and the Applications retry path, and prevents a new standard launch
until every installed artifact matches. Realtime artifacts remain subject to
the existing stopped-host requirement. Neither selection restarts a process.

## Isolated checks observed during this change

The diagnostic sandbox used a new filesystem view with an empty home, private
IPC/PID/network namespaces, and a synthetic `/dev`. It hid the actual machine
files and devices. A local tool-table owner retained a sentinel entry while
the installed `/usr/bin/rs274` executable loaded the staged library. The
checks covered `--help`, a no-motion parse, two concurrent parsers, shared-user
close, standalone close, repeated close, and an injected `ENOMEM` on the
standalone table allocation. The sentinel, file inode, size and mtime were
unchanged, and the parser traces contained no `.tool.mmap` path accesses.
Only the isolated shared owner removed its file when it closed.

The injected allocation failure returned an error identifying the failed
allocation and the retry action; it did not fall back to a named shared file.
The Rust launcher build and six existing tests returned without errors.
These are assistant-arranged software check results, not machine evidence.

Retained software artifacts from the investigation are under
`/tmp/dmc2-tooldata-fix.jm0uey/`. The original library SHA-256 is
`446365bfc12f3ccaf65cf87aa7e38688b44f3d2279f516094ae3a2d51b991226`;
the staged corrected library SHA-256 is
`4a9284bf35f1edc6cfbb1241ea09c2c91c5bc80b68c59a6685f187babbc9996d`.

## Operator control and evidence boundaries

This change adds no restart, process termination, reset, fault clear, homing,
spindle command, or motion. The UI Abort, Clear Fault and Pendant Mode code
and bindings are unchanged. Library installation does not revive the task
process that already crashed or certify its current UI recovery path.

The earlier statement that live HAL was gone was unsupported: `halcmd` ran
inside a separate diagnostic IPC namespace and showed an empty instance.
`rtapi_app` was subsequently observed still running. The core and lifecycle
record do establish the task process loss. Process presence is not evidence
of physical machine state.

Checks must hide the actual home directory, hardware devices and live IPC
before invoking a parser. Tests, successful builds, symbol comparisons and
syscall traces may expose regressions or describe observed software behavior;
they do not prove machine safety or universal UI recoverability. A plugin,
explicit output-file argument, or deliberate same-user file write is outside
this tool-storage isolation boundary.
