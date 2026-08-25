# DMC2 programs

LinuxCNC G-code files can be stored here. No machine program is installed or
started automatically by the live configuration.

`dmc2_spindle_test.ngc` is the reusable clockwise spindle-test operation. It
does not contain a fixed test speed: call it from LinuxCNC MDI or another
program as `o<dmc2_spindle_test> call [RPM]`. The requested RPM must be within
the configured spindle range. The operation confirms stopped feedback, issues
the direct `S#1 M3` request, waits for H100 running and at-speed feedback,
issues `M5`, and waits for stopped feedback. It contains no axis command and
is never started automatically.

`log-top-25mm-hardwood.ngc` is the explicitly confirmed first hardwood
surfacing program. It requires an exact X300/Y173/Z135 machine-home start and
cuts the measured log rectangle X34..262/Y0..173, leaving 34 mm off the
physical right side and 38 mm off the physical left side of the former full-X
plane. It commands the verified M3/CW spindle at 18,000 RPM and 1,500 mm/min,
makes six 4 mm roughing passes at 8 mm stepover, then one 1 mm finishing pass
at 2 mm stepover, and retracts straight up after reaching machine Z110. It is
never started automatically.
