# DMC2 script contract v1

This document is the exact contract for loading a LinuxCNC machine-code file
without adding a catalog row or changing application code. The authoritative
parser is the Rust `dmc2ctl` script module. AXIS invokes that parser and
decodes its versioned output; the AXIS Python integration does not parse or
reinterpret file headers.

## Header grammar

The optional header uses LinuxCNC parenthesized comments, so LinuxCNC can read
the same file directly:

```ngc
%
(DMC2 SCRIPT 1)
(DMC2 EFFECTS axis-motion;probe-power)
(DMC2 REQUIRES running-session;estop-clear;machine-on;interpreter-idle)
(DMC2 RECOVERY abort-task)
(DMC2 END)
G21 G90
...
%
```

The grammar is deliberately exact:

- `(DMC2 SCRIPT 1)` starts the v1 header.
- Leading and trailing whitespace around a complete header line is ignored;
  whitespace inside a field value is not.
- `EFFECTS`, `REQUIRES`, and `RECOVERY` must each occur exactly once, in any
  order, before `(DMC2 END)`.
- `EFFECTS` and `REQUIRES` contain one or more exact lowercase values separated
  by semicolons. Whitespace around values is not accepted.
- Duplicate fields, duplicate list values, empty values, unknown values,
  unknown fields inside a started header, malformed `(DMC2 SCRIPT ...)` magic,
  and an unterminated header are errors. A rejected file is not forwarded to
  AXIS.
- Blank lines, `%`, semicolon comments, and complete parenthesized comments may
  precede the magic line. Ordinary descriptive `(DMC2 ...)` comments are not
  header syntax. If executable code appears first, the file is headerless and
  receives the conservative contract below.
- Inspection examines at most the first 128 physical lines. A header line may
  contain at most 4096 bytes.
- The selected path must resolve to an existing regular file. The canonical
  path is the identity passed onward to LinuxCNC.

## Closed values

`EFFECTS` accepts:

- `axis-motion`
- `spindle`
- `probe-power`
- `coolant`
- `tool-change`
- `digital-output`
- `coordinate-state`
- `external-command`
- `unclassified-machine-code`

Effects describe possible behavior. They do not authorize an action and do
not cause the loader to issue a command.

`REQUIRES` accepts:

- `running-session`
- `estop-clear`
- `machine-on`
- `interpreter-idle`
- `all-homed`

AXIS observes these requirements immediately before forwarding the operator's
explicit Run or initial Step request. Continuing Step in an active AUTO
program retains the other checks but does not require interpreter-idle.
`dmc2ctl execute-file PATH` evaluates the same typed
requirements before loading and again before submitting Run. A requirement
omitted from a header is intentionally not added by the loader. LinuxCNC's own
interpreter and machine checks remain authoritative.

`RECOVERY` accepts the shared recovery-catalog slugs:

- `recheck-source`
- `clear-controller`
- `restore-pendant`
- `release-limit`
- `abort-task`
- `restore-machine`
- `reset-spindle`
- `relaunch-application`
- `establish-position`

This value records the script's declared default recovery class. A more
specific fault diagnosed by LinuxCNC or the native DMC2 task monitor retains
its own typed recovery class and visible UI path.

There is no separate detection language in v1. Probe waits, contact inputs,
result capture, and other machine semantics stay in the LinuxCNC program that
executes them. Duplicating those conditions in a loader header would create a
second implementation that could disagree with the actual program. A future
loader-owned capture mechanism requires a new, explicitly versioned contract.

## Headerless files

A regular file with no DMC2 header is accepted without a code or catalog
change and receives exactly this conservative contract:

```text
effects=unclassified-machine-code
prerequisites=running-session;estop-clear;machine-on;interpreter-idle;all-homed
recovery=abort-task
```

This fallback makes the arbitrary-file hook usable while preventing an
unclassified file from silently weakening the normal machine-state checks.

## Operator and programmatic paths

In AXIS, use the existing **File → Open** dialog or toolbar Open control. The
integration performs a read-only Rust inspection and then forwards the exact
canonical file to stock AXIS. It re-inspects that selected path immediately
before every explicit Run or Step and compares the byte count and deterministic
64-bit FNV-1a content revision recorded at File Open. If the content changed,
execution is blocked and the operator is told to use File Open again; AXIS Reload or
an edited file cannot silently reuse stale header metadata. The revision is a
change-detection token, not a cryptographic authenticity claim. The loader
never runs, homes, resets, clears, restarts, or moves the machine. The existing
AXIS Run and Step controls remain explicit execution actions. Their blocking
boundary is installed before optional extensions: an unavailable loader or
guard leaves execution blocked, with recovery controls installed independently.

The compiled CLI exposes the same path contract:

```text
dmc2ctl inspect-file PATH
dmc2ctl load-file PATH
dmc2ctl execute-file PATH
```

`inspect-file` is read-only. `load-file` only loads and confirms the exact file
reported by LinuxCNC. `execute-file` is the explicit load-and-run command. No
command is inferred from inspecting or selecting a file.

The Rust inspector parses and hashes one read stream. `load-file` and
`execute-file` retain that contract and revalidate it before/after loading and
immediately before Run, including after waiting for AUTO mode. A changed
revision is rejected with a File Open/review/retry action. These checks detect
changes at those boundaries; they do not lock another process out of editing
the file during LinuxCNC execution.

Successful `inspect-file` output is the ASCII protocol
`DMC2_SCRIPT_CONTRACT_V1`, with exactly these ordered `key=value` fields:

```text
format
path_hex
content_bytes
content_fnv1a64
contract_source
effects
prerequisites
recovery_class
recovery_slug
```

`path_hex` is the canonical operating-system path encoded byte-for-byte as
lowercase hexadecimal. The AXIS bridge rejects missing, extra, reordered,
unknown, duplicated, noncanonical, or internally inconsistent fields.

## Failure and evidence boundaries

- A parse, inspection-process, protocol, or path-identity failure is presented
  as a typed script-inspection fault. Selecting a corrected file through the
  visible AXIS Open control is its source-recheck path; Pendant Mode remains
  available.
- A stock load-submission exception is presented as a typed load fault with
  visible Abort, Clear Fault, File Open, and Pendant Mode recovery actions.
- Before Run or Step is forwarded, the path selected by AXIS must match LinuxCNC's
  returned loaded-file status. A selection or successful parser result is not
  treated as consumer acceptance.
- Machine readiness, interpreter-idle state, and homing are checked only when
  required by the active typed contract. Each failed requirement maps to its
  existing visible recovery class.
- Once execution has been submitted, LinuxCNC interpreter/task errors continue
  through the sole native task-monitor error channel. The UI presents their
  source file, line, typed cause, and recovery path. The loader does not consume
  or race that channel.
- Parsing, compilation, local tests, UI selection, and command submission are
  separate evidence boundaries. None proves physical motion, probe contact,
  script completion, or recovery on the machine.
