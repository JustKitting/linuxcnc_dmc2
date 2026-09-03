# LinuxCNC runtime logs

This directory is the durable parent for LinuxCNC-owned diagnostic output.
Runtime payloads remain ignored by Git.

- `error-channel.tsv` preserves the native LinuxCNC error-channel records.
- `diagnostics.tsv` is the checksummed, append-only, self-describing event
  stream. Every assertion and clearance records the diagnostic identity, plain
  cause, operator action, source domain, original raw value, and the complete
  coherent evidence snapshot captured with that value. Unknown values retain
  an explicit `UNKNOWN_<DOMAIN>(raw=<value>)` identity and are never guessed.
- `process-lifecycle.tsv` is the locked, checksummed, append-only lifecycle
  stream shared by `dmc2-process-supervisor` and
  `dmc2-session-supervisor`. It records catalogued roles and ownership,
  tracker/child/parent identity, exact invocation bytes, `/proc` process
  snapshots before and after execution, terminal cgroup counters and
  membership, monotonic lifetime, independently checked `waitid` and `wait4`
  status, exit code or terminating signal, core-dump policy/status, and
  resource usage.
- `process-backtrace-<pid>-<time>.txt` is a durable copy of a matching
  `/tmp/backtrace.<pid>` created by LinuxCNC's own `SIGSEGV`/`SIGFPE` handler.
- `process-core-<pid>-<time>.core` is an identity-checked, synchronized copy
  of a file-based kernel core retained before the owning tracker reaps that
  child. The lifecycle record states explicitly when policy, naming, absence,
  rejection, or an I/O failure prevents a copy.

This tracking is passive. It never restarts, stops, enables, disables, homes,
jogs, or otherwise changes the machine. LinuxCNC 2.9.10 catches `SIGINT` and
`SIGTERM` and later exits normally, so a parent wait status alone cannot name
those two delivered signals. Its fatal-signal handler likewise converts
`SIGSEGV` and `SIGFPE` into a normal exit, which is why the matching backtrace
header is preserved separately. If the supervisor and `milltask` are killed
simultaneously, the already-synchronized start record remains but no user-space
process can guarantee a final record after its own termination. The outer
session subreaper records direct-owner deaths and adopted descendants while it
remains alive. See `docs/process-lifecycle-tracking.md` for exact boundaries.
