"""Accepted live INI and HAL topology validation."""

import re

from .common import executable_hal_text, read_ini, require_float, require_text
from .paths import LIVE_DIR, PROJECT_ROOT as ROOT


def validate_live_ini() -> str:
    config = read_ini(LIVE_DIR / "dmc2.ini")
    required_sections = {
        "EMC", "DISPLAY", "TASK", "RS274NGC", "EMCMOT", "EMCIO", "HAL",
        "HALUI", "TRAJ", "KINS", "AXIS_X", "AXIS_Y", "AXIS_Z",
        "JOINT_0", "JOINT_1", "JOINT_2", "SPINDLE_0", "DMC2",
    }
    missing = sorted(required_sections - set(config.sections()))
    if missing:
        raise AssertionError(f"live INI missing sections: {missing}")

    require_text(config, "EMC", "MACHINE", "DMC2-MINI-PROVISIONAL")
    require_text(config, "DMC2", "MESA_IP", "192.168.1.121")
    require_text(config, "DMC2", "PENDANT_PORT", "/dev/ttyUSB0")
    require_text(
        config,
        "DISPLAY",
        "USER_COMMAND_FILE",
        "../python/dmc2_axis/axis_user_command.py",
    )
    require_float(config, "DMC2", "PULSES_PER_MM", 1000.0)
    require_float(config, "TRAJ", "MAX_LINEAR_VELOCITY", 30.0)
    require_float(config, "TRAJ", "MAX_LINEAR_ACCELERATION", 50.0)
    require_text(config, "TRAJ", "NO_FORCE_HOMING", "0")
    require_text(config, "RS274NGC", "SUBROUTINE_PATH", "./nc_files")
    require_text(
        config,
        "RS274NGC",
        "ON_ABORT_COMMAND",
        "o<dmc2_abort> call",
    )
    require_float(config, "SPINDLE_0", "MIN_FORWARD_VELOCITY", 6000.0)
    require_float(config, "SPINDLE_0", "MAX_FORWARD_VELOCITY", 24000.0)
    require_float(config, "SPINDLE_0", "MIN_REVERSE_VELOCITY", 6000.0)
    require_float(config, "SPINDLE_0", "MAX_REVERSE_VELOCITY", 24000.0)
    require_float(config, "DMC2", "SPINDLE_RATED_RPM", 24000.0)
    require_float(config, "DMC2", "SPINDLE_MINIMUM_RPM", 6000.0)
    require_float(config, "DMC2", "SPINDLE_MAXIMUM_RPM", 24000.0)
    require_float(
        config,
        "DMC2",
        "SPINDLE_TEST_TRANSITION_TIMEOUT_SECONDS",
        30.0,
    )
    require_float(config, "DMC2", "SPINDLE_EXPECTED_F004_CENTIHZ", 4000.0)
    require_float(config, "DMC2", "SPINDLE_EXPECTED_F005_CENTIHZ", 4000.0)
    require_float(config, "DMC2", "SPINDLE_AT_SPEED_TOLERANCE_HZ", 1.0)

    profile = (
        ("X", 0, 300.0, 300.25),
        ("Y", 1, 173.0, 173.25),
        ("Z", 2, 135.0, 135.25),
    )
    for axis, joint, normal_max, switch_coordinate in profile:
        axis_section = f"AXIS_{axis}"
        joint_section = f"JOINT_{joint}"
        require_float(config, axis_section, "MIN_LIMIT", 0.0)
        require_float(config, axis_section, "MAX_LIMIT", normal_max)
        require_float(config, axis_section, "MAX_VELOCITY", 30.0)
        require_float(config, axis_section, "MAX_ACCELERATION", 50.0)

        require_float(config, joint_section, "MIN_LIMIT", 0.0)
        require_float(config, joint_section, "MAX_LIMIT", switch_coordinate)
        require_float(config, joint_section, "SCALE", 1000.0)
        require_float(config, joint_section, "MAX_VELOCITY", 30.0)
        require_float(config, joint_section, "MAX_ACCELERATION", 50.0)
        require_float(config, joint_section, "FERROR", 0.050)
        require_float(config, joint_section, "MIN_FERROR", 0.010)
        require_float(config, joint_section, "STEPGEN_MAX_VEL", 33.0)
        require_float(config, joint_section, "STEPGEN_MAX_ACC", 0.0)
        require_float(config, joint_section, "HOME", normal_max)
        require_float(config, joint_section, "HOME_OFFSET", switch_coordinate)
        require_float(config, joint_section, "HOME_SEARCH_VEL", 5.0)
        require_float(config, joint_section, "HOME_LATCH_VEL", 0.25)
        require_float(config, joint_section, "HOME_FINAL_VEL", 0.25)
        require_text(config, joint_section, "HOME_IGNORE_LIMITS", "YES")
        require_text(config, joint_section, "HOME_SEQUENCE", str(joint))
        if config.getfloat(joint_section, "HOME_OFFSET") - config.getfloat(
            joint_section, "HOME"
        ) != 0.25:
            raise AssertionError(f"{joint_section} final home move is not -0.25 mm")

    return (
        "live INI exactly matches 1000 pulses/mm, X 0..300, Y 0..173, "
        "Z 0..135, and the accepted X/Y/Z homing sequence"
    )


def validate_live_hal() -> list[str]:
    machine_path = LIVE_DIR / "machine.hal"
    pendant_path = LIVE_DIR / "pendant.hal"
    mesa_path = ROOT / "live" / "hal" / "mesa_status_sources.hal"
    machine = executable_hal_text(machine_path)
    pendant = executable_hal_text(pendant_path)
    mesa = executable_hal_text(mesa_path)
    combined = "\n".join((machine, pendant, mesa))

    required_machine_tokens = (
        'loadrt hm2_eth board_ip=[DMC2]MESA_IP config="num_encoders=0 num_stepgens=3 num_pwmgens=0 num_3pwmgens=0 num_inmuxs=1 num_pktuarts=1"',
        'loadrt hm2_modbus ports="hm2_7i95.0.pktuart.0" mbccbs="../../h100_modbus/maps/live/h100-spindle.mbccb"',
        "loadrt h100_spindle",
        "loadrt dmc2_rt",
        "loadrt mux2 names=dmc2-x-feedback-loss-guard,dmc2-y-feedback-loss-guard,dmc2-z-feedback-loss-guard",
        "loadrt timedelay names=dmc2-servo-startup-delay",
        "loadrt oneshot names=dmc2-puck-test-window",
        "dmc2-probe-power-gate",
        "dmc2-probe-power-source-or",
        "dmc2-puck-test-reset-or",
        "setp dmc2-puck-test-window.width 300.0",
        "setp dmc2-puck-test-window.retriggerable false",
        "setp dmc2-puck-test-window.rising true",
        "setp dmc2-puck-test-window.falling false",
        "setp hm2_7i95.0.inmux.00.fast_scans 1",
        "setp hm2_7i95.0.watchdog.timeout_ns 100000000",
        "setp hm2_7i95.0.dpll.01.timer-us -100",
        "setp hm2_7i95.0.stepgen.timer-number 1",
        "setp dmc2-servo-startup-delay.on-delay 0.100",
        "net dmc2-servo-startup-ready dmc2-servo-startup-delay.out",
        "net dmc2-limit-x-live => joint.0.home-sw-in dmc2-hard-limit-x-enabled.in0",
        "net dmc2-limit-y-live => joint.1.home-sw-in dmc2-hard-limit-y-enabled.in0",
        "net dmc2-limit-z-live => joint.2.home-sw-in dmc2-hard-limit-z-enabled.in0",
        "dmc2-hard-limit-x-enabled.out => joint.0.pos-lim-sw-in",
        "dmc2-hard-limit-y-enabled.out => joint.1.pos-lim-sw-in",
        "dmc2-hard-limit-z-enabled.out => joint.2.pos-lim-sw-in",
        "net dmc2-spindle-at-speed h100-spindle.at-speed => spindle.0.at-speed motion.digital-in-02",
        "net dmc2-spindle-running h100-spindle.running => motion.digital-in-03",
        "dmc2-pendant-control.jog-stop => motion.jog-stop",
        "dmc2-jog-stop-immediate-all.out => motion.jog-stop-immediate",
        "dmc2-controller-motor-0-toward-limit => dmc2-own-toward-block-m0.in1",
        "dmc2-controller-motor-1-toward-limit => dmc2-own-toward-block-m1.in1",
        "dmc2-controller-motor-2-toward-limit => dmc2-own-toward-block-m2.in1",
        "dmc2-controller-motor-0-command-enable => dmc2-command-blocked-m0.in0",
        "dmc2-controller-motor-1-command-enable => dmc2-command-blocked-m1.in0",
        "dmc2-controller-motor-2-command-enable => dmc2-command-blocked-m2.in0",
        "joint.0.motor-pos-cmd => hm2_7i95.0.stepgen.01.position-cmd dmc2-x-feedback-loss-guard.in1",
        "hm2_7i95.0.stepgen.01.position-fb => dmc2-x-feedback-loss-guard.in0",
        "dmc2-x-feedback-loss-guard.out => joint.0.motor-pos-fb",
        "joint.1.motor-pos-cmd => hm2_7i95.0.stepgen.00.position-cmd dmc2-y-feedback-loss-guard.in1",
        "hm2_7i95.0.stepgen.00.position-fb => dmc2-y-feedback-loss-guard.in0",
        "dmc2-y-feedback-loss-guard.out => joint.1.motor-pos-fb",
        "joint.2.motor-pos-cmd => hm2_7i95.0.stepgen.02.position-cmd dmc2-z-feedback-loss-guard.in1",
        "hm2_7i95.0.stepgen.02.position-fb => dmc2-z-feedback-loss-guard.in0",
        "dmc2-z-feedback-loss-guard.out => joint.2.motor-pos-fb",
        "hm2_7i95.0.packet-error => dmc2-x-feedback-loss-guard.sel dmc2-y-feedback-loss-guard.sel dmc2-z-feedback-loss-guard.sel",
        "hm2_7i95.0.watchdog.has_bit => estop-latch.0.fault-in",
        "spindle.0.on => h100-spindle.run-request",
        "spindle.0.forward => h100-spindle.forward-request",
        "spindle.0.reverse => h100-spindle.reverse-request",
        "spindle.0.speed-out-abs => h100-spindle.speed-command-rpm",
        "hm2_modbus.0.fault => h100-spindle.link-fault",
        "h100-spindle.main-control => hm2_modbus.0.h100.main-control",
        "h100-spindle.given-frequency => hm2_modbus.0.h100.given-frequency",
        "h100-spindle.at-speed => spindle.0.at-speed",
        "h100-spindle.speed-feedback-rpm => spindle.0.speed-in",
        "h100-spindle.fault-latched => spindle.0.amp-fault-in",
        "setp hm2_modbus.0.suspend false",
    )
    missing = [token for token in required_machine_tokens if token not in machine]
    if missing:
        raise AssertionError(f"live machine HAL mapping missing: {missing}")

    required_disabled_monitors = tuple(
        f"hm2_modbus.0.command.{index:02d}.disabled => "
        f"h100-spindle.command-disabled{index}"
        for index in range(13)
    )
    missing = [
        token for token in required_disabled_monitors if token not in machine
    ]
    if missing:
        raise AssertionError(
            f"H100 command-disable monitoring is incomplete: {missing}"
        )

    forbidden_direct_limits = (
        "dmc2-limit-x-live => joint.0.home-sw-in joint.0.pos-lim-sw-in",
        "dmc2-limit-y-live => joint.1.home-sw-in joint.1.pos-lim-sw-in",
        "dmc2-limit-z-live => joint.2.home-sw-in joint.2.pos-lim-sw-in",
    )
    present = [token for token in forbidden_direct_limits if token in machine]
    if present:
        raise AssertionError(
            "raw limits still bypass the exact pendant bounce gate: "
            f"{present}"
        )

    order = (
        "addf hm2_7i95.0.read servo-thread",
        "addf dmc2-safety-limit-x servo-thread",
        "source hal/mesa_status_sources.hal",
        "addf dmc2-pendant-control.update servo-thread",
        "addf dmc2-own-toward-block-m0 servo-thread",
        "addf dmc2-jog-stop-all servo-thread",
        "addf dmc2-jog-stop-immediate-all servo-thread",
        "addf dmc2-hard-limit-x-enabled servo-thread",
        "addf dmc2-x-feedback-loss-guard servo-thread",
        "addf dmc2-z-feedback-loss-guard servo-thread",
        "addf motion-command-handler servo-thread",
        "addf motion-controller servo-thread",
        "addf h100-spindle servo-thread",
        "addf hm2_modbus.0.process servo-thread",
        "addf dmc2-probe-power-gate servo-thread",
        "addf dmc2-puck-test-reset-or servo-thread",
        "addf dmc2-puck-test-window servo-thread",
        "addf dmc2-probe-power-source-or servo-thread",
        "addf hm2_7i95.0.write servo-thread",
        "addf dmc2-servo-startup-delay servo-thread",
    )
    positions = [machine.index(token) for token in order]
    if positions != sorted(positions):
        raise AssertionError("live servo-thread read/safety/motion/write order changed")

    required_pendant_tokens = (
        "../native/bin/dmc2-serial-bridge",
        "../native/bin/dmc2-task-monitor",
        "--error-journal ../var/log/linuxcnc/error-channel.tsv",
        "dmc2-pendant.snapshot-generation",
        "dmc2-pendant-control.snapshot-generation",
        "dmc2-task-monitor.task-heartbeat => dmc2-pendant-control.task-heartbeat",
        "dmc2-task-monitor.machine-on => dmc2-pendant-control.machine-on",
        "dmc2-task-monitor.axis-0-stopped => dmc2-pendant-control.axis-0-stopped",
        "dmc2-pendant-control.motor-0-count",
        "dmc2-pendant-control.motor-1-count",
        "dmc2-pendant-control.motor-2-count",
        "dmc2-y-stepgen-position-feedback => dmc2-pendant-control.motor-0-position-feedback",
        "dmc2-x-stepgen-position-feedback => dmc2-pendant-control.motor-1-position-feedback",
        "dmc2-z-stepgen-position-feedback => dmc2-pendant-control.motor-2-position-feedback",
        "dmc2-pendant-control.motor-0-limit-latched",
        "dmc2-pendant-control.motor-1-limit-latched",
        "dmc2-pendant-control.motor-2-limit-latched",
        "dmc2-pendant-control.external-enable",
        "dmc2-pendant-control.heartbeat",
        "dmc2-pendant-control.motor-0-command-enable",
        "dmc2-pendant-control.motor-1-command-enable",
        "dmc2-pendant-control.motor-2-command-enable",
        "dmc2-pendant-control.motor-0-toward-limit",
        "dmc2-pendant-control.motor-1-toward-limit",
        "dmc2-pendant-control.motor-2-toward-limit",
        "dmc2-pendant-control.mesa-watchdog-has-bit",
        "dmc2-pendant-control.software-watchdog-ok",
        "dmc2-pendant-control.servo-thread-ready",
        "dmc2-pendant-control.mesa-packet-error",
        "dmc2-pendant-control.control-available",
        "dmc2-pendant-control.control-ready",
        "dmc2-pendant-control.estop-reset-request",
        "dmc2-pendant-control.machine-on-request",
        "hm2_7i95.0.packet-error-total => dmc2-pendant-control.mesa-packet-error-total",
        "hm2_7i95.0.packet-error-exceeded => dmc2-pendant-control.mesa-packet-error-exceeded",
        "dmc2-pendant-control.axis-0-increment-plus => halui.axis.x.increment-plus",
        "dmc2-pendant-control.axis-0-increment-minus => halui.axis.x.increment-minus",
        "dmc2-pendant-control.axis-0-increment => halui.axis.x.increment",
        "dmc2-pendant-control.axis-jog-speed => halui.axis.jog-speed",
        "dmc2-pendant-control.joint-0-increment-plus => halui.joint.0.increment-plus",
        "dmc2-pendant-control.joint-0-increment-minus => halui.joint.0.increment-minus",
        "dmc2-pendant-control.joint-0-increment => halui.joint.0.increment",
        "dmc2-pendant-control.joint-jog-speed => halui.joint.jog-speed",
    )
    missing = [token for token in required_pendant_tokens if token not in pendant]
    if missing:
        raise AssertionError(f"live pendant HAL mapping missing: {missing}")
    forbidden_live_python = (
        "python3",
        "nano_hal_bridge.py",
        "linuxcnc_pendant_control.py",
    )
    present = [token for token in forbidden_live_python if token in pendant]
    if present:
        raise AssertionError(
            f"live pendant path still grants Python control authority: {present}"
        )
    if (
        "net dmc2-controller-watchdog-ok => "
        "dmc2-pendant-control.software-watchdog-ok"
    ) not in pendant:
        raise AssertionError("controller cannot verify realtime heartbeat watchdog")
    postgui = (ROOT / "live" / "hal" / "status_postgui.hal").read_text(
        encoding="utf-8"
    )
    if (
        "net dmc2-probe-program-running halui.program.is-running => "
        "dmc2-probe-power-gate.in1"
    ) not in postgui:
        raise AssertionError("OUT5 is not gated by LinuxCNC program-running state")
    required_panel_test_tokens = (
        "net dmc2-puck-test-start pyvcp.puck-test-start => dmc2-puck-test-window.in",
        "net dmc2-puck-test-stop pyvcp.puck-test-stop => dmc2-puck-test-reset-or.in1",
        "net dmc2-puck-test-active => pyvcp.puck-test-active",
        "net dmc2-puck-test-time-left => pyvcp.puck-test-time-left",
    )
    missing = [token for token in required_panel_test_tokens if token not in postgui]
    if missing:
        raise AssertionError(f"unhomed panel puck test is incomplete: {missing}")
    if not postgui.rstrip().endswith(
        "setp dmc2-pendant-control.ui-ready true"
    ):
        raise AssertionError("AXIS post-GUI readiness is not the final HAL operation")

    required_mesa_tokens = (
        "raw-input-11", "raw-input-09", "raw-input-10", "raw-input-00",
        "raw-input-01", "ssr.00.out-05",
        "net dmc2-puck-live    => motion.probe-input motion.digital-in-00",
        "setp hm2_7i95.0.ssr.00.invert-05 false",
        "net dmc2-probe-power-request motion.digital-out-00 => dmc2-probe-power-gate.in0",
        "net dmc2-probe-program-power-enabled dmc2-probe-power-gate.out => dmc2-probe-power-source-or.in0",
        "net dmc2-puck-test-active dmc2-puck-test-window.out => dmc2-probe-power-source-or.in1",
        "net dmc2-probe-power-enabled dmc2-probe-power-source-or.out => hm2_7i95.0.ssr.00.out-05 motion.digital-in-01",
        "net dmc2-puck-live => dmc2-puck-test-reset-or.in0",
        "net dmc2-puck-test-reset dmc2-puck-test-reset-or.out => dmc2-puck-test-window.reset",
        "net dmc2-puck-test-time-left dmc2-puck-test-window.time-left",
    )
    missing = [token for token in required_mesa_tokens if token not in mesa]
    if missing:
        raise AssertionError(f"Mesa status mapping missing: {missing}")
    if "sets dmc2-probe-power-enabled" in mesa:
        raise AssertionError("OUT5 still has a second static HAL writer")
    unsafe_direct_spindle_routes = (
        "spindle.0.on => hm2_modbus.0.h100.main-control",
        "spindle.0.speed-out => hm2_modbus.0.h100.given-frequency",
        "spindle.0.speed-out-abs => hm2_modbus.0.h100.given-frequency",
    )
    present = [token for token in unsafe_direct_spindle_routes if token in combined]
    if present:
        raise AssertionError(
            f"LinuxCNC bypasses the H100 sequencer: {present}"
        )
    ssr_outputs = set(re.findall(r"hm2_7i95\.0\.ssr\.00\.out-[0-9]{2}", combined))
    if ssr_outputs != {"hm2_7i95.0.ssr.00.out-05"}:
        raise AssertionError(f"unexpected Mesa SSR output mapping: {sorted(ssr_outputs)}")

    return [
        "live HAL preserves X=stepgen1/IN11, Y=stepgen0/IN9, Z=stepgen2/IN10",
        "old directional limit gate stops pendant jogs in realtime and preserves negative bounce",
        "Mesa read, realtime limit gate, motion, and Mesa write are ordered",
        "startup waits for Mesa writes, clears a stale watchdog bite, and settles limit latches",
        "IN0 feeds LinuxCNC probe/digital input; OUT5 has program and finite panel-test gates",
        "the unhomed panel test stops on IN0 contact or STOP and cannot exceed 300 seconds",
        "spindle control is sequenced through H100 readback with the verified 24000 RPM to 400.0 Hz mapping and 6000 RPM software minimum",
        "the physical drive-enable SSR remains absent",
    ]
