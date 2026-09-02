# LinuxCNC runtime logs

This directory is the durable parent for LinuxCNC-owned diagnostic output.
Runtime payloads remain ignored by Git.

- `error-channel.tsv` preserves the native LinuxCNC error-channel records.
- `diagnostics.tsv` is the checksummed, append-only, self-describing event
  stream. Every assertion and clearance records the diagnostic identity, plain
  cause, operator action, source domain, original raw value, and the complete
  coherent evidence snapshot captured with that value. Unknown values retain
  an explicit `UNKNOWN_<DOMAIN>(raw=<value>)` identity and are never guessed.
