# Native LinuxCNC control path

The machine project uses four compiled interfaces:

- `dmc2-serial-bridge`: bounded Nano P3 parsing and coherent HAL publication.
- `dmc2-task-monitor`: read-only native LinuxCNC status, real task heartbeat,
  source-derived code/error classification, transition logging, and HAL
  diagnostics.
- `dmc2_rt.so`: the no-`std` servo-thread supervisor, limit/bounce policy, and
  native realtime axis/joint wheel-jog command and acknowledgement transport.
- `dmc2ctl`: the typed command-line client for the already-running LinuxCNC
  NML session. Its operations come from `config/operations.tsv`; program load
  and program run are deliberately separate commands.

`scripts/build_native.sh` builds the complete release with warnings denied and
stages the four userspace binaries under `native/bin`. After the realtime
module is installed, `scripts/verify.sh` runs the isolated real-LinuxCNC motion
acceptance matrix, including X/Y/Z modeled limit-stop, bounce, latch-reset, and
raw-limit-held operator release paths plus production AXIS journal-reader
checks. A live launch automatically invokes the
transactional module synchronizer when either verified realtime module differs
from LinuxCNC's fixed module directory; no separate operator install step is
required.

The build and launcher are locked to LinuxCNC 2.9.10. The task monitor uses its
native NML status and error-channel interfaces only while running as part of
LinuxCNC; it has no separate fake-validation mode.

The live HAL contains no Python process with command authority. Python remains
only in stock AXIS presentation integration.
