# LinuxCNC runtime logs

This directory is the durable parent for LinuxCNC-owned diagnostic output.
Runtime payloads remain ignored by Git.

- `error-channel.tsv` preserves the native LinuxCNC error-channel records.
- `diagnostics.tsv` is the checksummed, append-only, self-describing event
  stream. Every assertion and clearance records the diagnostic identity, plain
  cause, operator action, source domain, original raw value, and the complete
  coherent evidence snapshot captured with that value. Unknown values retain
  an explicit `UNKNOWN_<DOMAIN>(raw=<value>)` identity and are never guessed.
- `milltask-lifecycle.tsv` is the checksummed, append-only process-lifecycle
  stream written by `dmc2-milltask-supervisor`. It records the supervisor and
  child PIDs, exact invocation bytes, monotonic lifetime, raw kernel wait
  status, exit code or terminating signal, and the core-dump status bit.
- `milltask-backtrace-<pid>-<time>.txt` is a durable copy of a matching
  `/tmp/backtrace.<pid>` created by LinuxCNC's own `SIGSEGV`/`SIGFPE` handler.

This tracking is passive. It never restarts, stops, enables, disables, homes,
jogs, or otherwise changes the machine. LinuxCNC 2.9.10 catches `SIGINT` and
`SIGTERM` and later exits normally, so a parent wait status alone cannot name
those two delivered signals. Its fatal-signal handler likewise converts
`SIGSEGV` and `SIGFPE` into a normal exit, which is why the matching backtrace
header is preserved separately. If the supervisor and `milltask` are killed
simultaneously, the already-synchronized start record remains but no user-space
process can guarantee a final record after its own termination.
