# DMC2 programs

LinuxCNC G-code files can be stored here. No machine program is installed or
started automatically by the live configuration.

Any existing regular LinuxCNC machine-code file can be selected through
AXIS **File → Open** or passed to `dmc2ctl inspect-file`, `load-file`, or
`execute-file` without adding a catalog row or changing code. A file may carry
the exact typed DMC2 v1 header; a headerless file receives the conservative
machine-on, interpreter-idle, and all-homed contract. The complete grammar and
failure behavior are defined in `docs/script-contract.md`.

`dmc2_spindle_test.ngc` is the reusable clockwise spindle-test operation. It
does not contain a fixed test speed: call it from LinuxCNC MDI or another
program as `o<dmc2_spindle_test> call [RPM]`. The requested RPM must be within
the configured spindle range. The operation confirms stopped feedback, issues
the direct `S#1 M3` request, requires H100 direction feedback to confirm the
physically verified clockwise state, waits for at-speed feedback, issues `M5`,
and waits for stopped feedback. It contains no axis command and is never
started automatically.

`log-top-25mm-hardwood.ngc` is the explicitly confirmed first hardwood
surfacing program. It requires an exact X300/Y173/Z135 machine-home start and
cuts the measured log rectangle X34..262/Y0..173, leaving 34 mm off the
physical right side and 38 mm off the physical left side of the former full-X
plane. It commands the verified M3/CW spindle at 18,000 RPM and 1,500 mm/min,
and refuses its first cutting move unless the H100 confirms both the clockwise
direction selection and requested speed. It makes six 4 mm roughing passes at
8 mm stepover, then one 1 mm finishing pass at 2 mm stepover, and retracts
straight up after reaching machine Z110. It is never started automatically.
