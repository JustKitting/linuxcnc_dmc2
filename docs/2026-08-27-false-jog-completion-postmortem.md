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

The old test corpus and obsolete reference/validation programs were removed.
The remaining verifier builds the release with warnings denied and runs one
isolated real-LinuxCNC acceptance program:

1. It refuses to run while another `rtapi_app` is active.
2. It byte-checks the deployed serial bridge, task monitor, launcher, and
   realtime module against the release build.
3. It starts LinuxCNC 2.9.10 with real `motmod` and a software step generator.
4. A PTY sends raw P3 packets through the deployed production serial bridge.
5. The deployed production `dmc2_rt.so` runs in the LinuxCNC servo thread.
6. The fixture sources the same pendant-input and motion-consumer HAL contracts
   as the live profile.
7. It reads LinuxCNC-owned `joint.N.motor-pos-cmd` and downstream step-generator
   counts; the runner cannot write those results.
8. Twenty data-driven moves cover X, Y, Z; x1, x10, x100; both directions; and
   consecutive X/x1 input. Every selected joint must move the expected distance
   within the accepted 20% manual-jog tolerance, and every unselected joint must
   remain still.
9. Three additional cases assert modeled X, Y, and Z raw and latched limits
   during a real LinuxCNC jog. Each retains the raw input through the complete
   automatic backoff, requires the production release-wait state without a
   fault, verifies LinuxCNC's native hard-limit input remains masked, sends one
   real x1 pendant detent away from the attributed limit, then clears the raw
   input and requires latch reset and return to ready.
10. The resulting real LinuxCNC error-channel records must parse successfully
    through the exact journal reader used by AXIS. Transport-success codes are
    generated from the pinned LinuxCNC 2.9.10 source and carried in the journal
    schema instead of being duplicated in Python. The run rejects any native
    `joint N on limit switch error` record.

The acceptance fixture loads no HostMot2 or Mesa driver and therefore cannot
address the physical CNC.

## Evidence boundary

Passing `scripts/verify.sh` now proves that raw pendant packets traverse the
deployed bridge and realtime controller, are accepted by LinuxCNC 2.9.10 motion,
produce downstream software step counts for the covered movement matrix, and
complete the isolated modeled limit/bounce/latch-reset paths described above.

It does not prove physical pulse delivery, motor motion, physical limit wiring
or switch response, E-stop recovery, homing, probe behavior, or spindle
behavior. Those require separate explicitly authorized hardware tests. No
completion statement may silently promote isolated LinuxCNC evidence into
physical-machine evidence.

## Accountability rule

A passing producer, adapter, mock, or source model is never evidence that the
real consumer worked. A live contradiction invalidates the corresponding test
claim immediately. The critical consumer and its returned evidence must be
observed before the implementation is described as working.
