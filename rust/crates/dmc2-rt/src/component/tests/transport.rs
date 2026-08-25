//! Exact safe-start publication regression test.

use super::*;
use dmc2_core::halui::PulsePhase;
use dmc2_core::startup::{ControllerWatchdogPhase, MesaStartupPhase};
use dmc2_core::supervisor::Phase;

#[test]
fn every_realtime_output_has_an_explicit_safe_start_value() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    reset_mock_hal();
    assert_eq!(rtapi_app_main(), 0);

    for stem in [
        "motor-0-limit-reset",
        "motor-1-limit-reset",
        "motor-2-limit-reset",
        "motor-0-command-enable",
        "motor-1-command-enable",
        "motor-2-command-enable",
        "motor-0-toward-limit",
        "motor-1-toward-limit",
        "motor-2-toward-limit",
        "axis-0-increment-plus",
        "axis-1-increment-plus",
        "axis-2-increment-plus",
        "axis-0-increment-minus",
        "axis-1-increment-minus",
        "axis-2-increment-minus",
        "joint-0-increment-plus",
        "joint-1-increment-plus",
        "joint-2-increment-plus",
        "joint-0-increment-minus",
        "joint-1-increment-minus",
        "joint-2-increment-minus",
        "external-enable",
        "watchdog-enable",
        "heartbeat",
        "position-known",
        "control-available",
        "control-ready",
        "fault",
        "recovery-active",
        "jog-active",
        "bounce-active",
        "estop-reset-request",
        "machine-on-request",
        "jog-stop",
        "jog-stop-immediate",
    ] {
        assert!(!get_bit(&format!("dmc2-pendant-control.{stem}")), "{stem}");
    }
    assert!(get_bit("dmc2-pendant-control.position-unknown"));

    for stem in [
        "axis-0-increment",
        "axis-1-increment",
        "axis-2-increment",
        "joint-0-increment",
        "joint-1-increment",
        "joint-2-increment",
        "axis-jog-speed",
        "joint-jog-speed",
    ] {
        assert_eq!(
            get_float(&format!("dmc2-pendant-control.{stem}")),
            0.0,
            "{stem}"
        );
    }

    for (stem, expected) in [
        ("fault-code", 0),
        ("supervisor-phase", Phase::Idle as i32),
        ("mesa-phase", MesaStartupPhase::WaitServo as i32),
        (
            "controller-watchdog-phase",
            ControllerWatchdogPhase::WaitPrerequisites as i32,
        ),
        ("command-phase", PulsePhase::Idle as i32),
    ] {
        assert_eq!(
            get_s32(&format!("dmc2-pendant-control.{stem}")),
            expected,
            "{stem}"
        );
    }

    rtapi_app_exit();
}
