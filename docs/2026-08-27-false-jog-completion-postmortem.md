# False jog-completion postmortem

## Failure

The previous test suite asserted local Rust and HAL publication while supplying
the downstream success state itself. It therefore passed even though the live
pendant path could not reliably move LinuxCNC. Reporting that suite as evidence
of working motion was false.

The immediate live symptom was a retained `JOG_COUNT_MISMATCH`: the controller
requested a second increment, but the observed Mesa count and position did not
reach its target. The old tests could not expose this because they did not run
the real LinuxCNC motion consumer.

## Root cause

The test boundary ended before LinuxCNC `motmod`. Mocks and offline adapters
accepted the producer output and returned the expected acknowledgement and
feedback. Those tests proved only that code could publish values, not that
LinuxCNC consumed them or generated downstream steps.

The repository also accumulated thousands of lines of overlapping Python,
Rust, shell, reference-model, and simulation tests. Their volume obscured the
missing critical boundary.

## Correction

The mock-heavy corpus, offline acceptance executable, software-motion fixture,
and obsolete reference programs were removed. The remaining narrow unit checks
cover local parsers, catalogs, argument construction, error formatting, and
pure state transitions. They are development checks only and are never evidence
that LinuxCNC or the machine accepted an action.

## Evidence boundary

Formatting, compilation, unit checks, pin existence, and process existence do
not prove producer routing, LinuxCNC consumer acceptance, physical pulse
delivery, motor motion, limit wiring, E-stop recovery, homing, probing, UI
survival, or spindle behavior. No completion statement may promote one evidence
boundary into another.

## Accountability rule

A passing producer, adapter, mock, or source model is never evidence that the
real consumer worked. A live contradiction invalidates the corresponding test
claim immediately. The critical consumer and its returned evidence must be
observed before the implementation is described as working.
