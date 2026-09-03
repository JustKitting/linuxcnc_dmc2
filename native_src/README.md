# Native evidence boundary

`dmc2_signal_evidence.c` is the small ABI interposer required inside the
unchanged LinuxCNC C processes. Rust owns library validation, socket creation,
record parsing, lifecycle state, journaling, and outcome classification.

The interposer wraps only libc `signal()` registrations for SIGINT and SIGTERM,
writes one fixed-size async-signal-safe evidence packet with `MSG_NOSIGNAL`
before invoking the original handler, and delegates every other signal number
to libc. A missing reader therefore cannot deliver SIGPIPE to LinuxCNC. Its build
script denies warnings, restricts the dynamic export set to `signal`, checks
for a non-executable stack, requires byte-reproducible output, and stages the
same bytes beside the release process supervisor.
Before launch, Rust compares the library's ELF class, byte order, and machine
to the actual catalogued target executable and records both file identities;
matching the supervisor executable alone is not treated as sufficient.

The send is nonblocking and the socket buffer is finite. It is intentionally
best-effort so evidence capture cannot stall LinuxCNC; no delivery record is
not proof that no signal was delivered, and the Rust journal states that
boundary explicitly.

This is passive evidence capture. It sends no signal and performs no machine
control or recovery action.
