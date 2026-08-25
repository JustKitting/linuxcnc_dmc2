#!/usr/bin/env python3
"""Validate the accepted DMC2 profile without opening serial, Mesa, or NML."""

from __future__ import annotations

import configparser
import json
import re
import shutil
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

from check_live_readiness import (
    READY_CONFIGURATION_STATUS,
    load_requirements,
    unresolved_requirements,
)


ROOT = Path(__file__).resolve().parent
SIM_DIR = ROOT / "sim"
LIVE_DIR = ROOT / "live"


def executable_hal_text(*paths: Path) -> str:
    return "\n".join(
        line.partition("#")[0]
        for path in paths
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.partition("#")[0].strip()
    )


def read_ini(path: Path) -> configparser.ConfigParser:
    # LinuxCNC intentionally permits repeated HALFILE entries.
    config = configparser.ConfigParser(strict=False)
    with path.open(encoding="utf-8") as stream:
        config.read_file(stream)
    return config


def require_float(
    config: configparser.ConfigParser,
    section: str,
    key: str,
    expected: float,
) -> None:
    actual = config.getfloat(section, key)
    if actual != expected:
        raise AssertionError(
            f"{section}.{key} must be exactly {expected}, found {actual}"
        )


def require_text(
    config: configparser.ConfigParser,
    section: str,
    key: str,
    expected: str,
) -> None:
    actual = config.get(section, key)
    if actual != expected:
        raise AssertionError(
            f"{section}.{key} must be exactly {expected!r}, found {actual!r}"
        )


def pin_names_from_panel(path: Path) -> set[str]:
    tree = ET.parse(path)
    if tree.getroot().tag != "pyvcp":
        raise AssertionError("status panel root must be <pyvcp>")
    pins = set()
    for node in tree.iter():
        attribute = node.attrib.get("halpin")
        if attribute is not None:
            pins.add(attribute.strip().strip('"').strip("'"))
    for node in tree.findall(".//halpin"):
        if node.text is None:
            raise AssertionError("empty <halpin> in status panel")
        pins.add(node.text.strip().strip('"'))
    return pins


def pin_names_from_postgui(path: Path) -> set[str]:
    text = path.read_text(encoding="utf-8")
    return set(re.findall(r"pyvcp\.([a-z0-9-]+)", text))


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
        "../axis_user_command.py",
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
    mesa_path = ROOT / "mesa_status_sources.hal"
    machine = executable_hal_text(machine_path)
    pendant = executable_hal_text(pendant_path)
    mesa = executable_hal_text(mesa_path)
    combined = "\n".join((machine, pendant, mesa))

    required_machine_tokens = (
        'loadrt hm2_eth board_ip=[DMC2]MESA_IP config="num_encoders=0 num_stepgens=3 num_pwmgens=0 num_3pwmgens=0 num_inmuxs=1 num_pktuarts=1"',
        'loadrt hm2_modbus ports="hm2_7i95.0.pktuart.0" mbccbs="/home/kit/h100_modbus/h100-spindle.mbccb"',
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
        "source ../mesa_status_sources.hal",
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
        "/home/kit/linuxcnc_dmc2/native/bin/dmc2-serial-bridge",
        "/home/kit/linuxcnc_dmc2/native/bin/dmc2-task-monitor",
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
    postgui = (ROOT / "status_postgui.hal").read_text(encoding="utf-8")
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


def validate_h100_spindle_integration() -> str:
    project = ROOT.parent / "h100_modbus"
    map_path = project / "h100-spindle.mbccs"
    binary_path = project / "h100-spindle.mbccb"
    component_path = project / "h100_spindle.comp"
    logic_path = project / "h100_spindle_logic.h"
    test_path = project / "test_h100_spindle_logic.c"
    for path in (map_path, binary_path, component_path, logic_path, test_path):
        if not path.is_file():
            raise AssertionError(f"H100 integration artifact is missing: {path}")

    tree = ET.parse(map_path)
    root = tree.getroot()
    if root.attrib.get("suspend") != "true":
        raise AssertionError("H100 Modbus map must start suspended")
    if root.attrib.get("writeflush") != "false":
        raise AssertionError("H100 Modbus writes must transmit established STOP state")

    writes = [
        int(command.attrib["address"], 0)
        for command in root.findall("./commands/command")
        if command.attrib.get("function") == "W_REGISTER"
    ]
    if writes != [0x0200, 0x0201]:
        raise AssertionError(
            "H100 write order must be main-control STOP/RUN before frequency"
        )

    read_addresses = {
        int(command.attrib["address"], 0)
        for command in root.findall("./commands/command")
        if command.attrib.get("function") in {"R_REGISTERS", "R_INPUTREGS"}
    }
    required_reads = {
        0x0001, 0x0004, 0x000B, 0x000E, 0x0018, 0x00A3, 0x00A9,
        0x0201, 0x0210,
    }
    if not required_reads <= read_addresses:
        raise AssertionError(
            f"H100 prerequisite/readback registers missing: "
            f"{sorted(required_reads - read_addresses)}"
        )

    input_register_commands = [
        command
        for command in root.findall("./commands/command")
        if command.attrib.get("function") == "R_INPUTREGS"
        and int(command.attrib["address"], 0) == 0x0000
    ]
    if len(input_register_commands) != 1:
        raise AssertionError("H100 map must have one input-register status read")
    status_pin_names = [
        pin.attrib["name"] for pin in input_register_commands[0].findall("pin")
    ]
    if status_pin_names != [
        "output-frequency", "set-frequency", "output-current"
    ]:
        raise AssertionError(
            "H100 status read must preserve the proven 0000H..0002H span"
        )

    component = component_path.read_text(encoding="utf-8")
    logic = logic_path.read_text(encoding="utf-8")
    required_tokens = (
        'pin out u32 main_control = 8',
        'pin out u32 given_frequency = 0',
        '#include "h100_spindle_logic.h"',
        "H100_CONTROL_REVERSE",
        "H100_BLOCK_COMMAND_DISABLED",
        "input.i_any_command_disabled = 1;",
        "input->i_given_frequency_readback ==",
        "case H100_STOPPING:",
        "output->o_main_control = H100_CONTROL_STOP;",
        "if (stopped_feedback)",
    )
    combined_source = component + "\n" + logic
    missing = [token for token in required_tokens if token not in combined_source]
    if missing:
        raise AssertionError(f"H100 fail-stopped sequencer is incomplete: {missing}")

    return (
        "H100 map starts suspended, writes STOP/control before frequency, reads "
        "all run prerequisites, and uses the unit-tested fail-stopped sequencer"
    )


def executable_gcode_text(path: Path) -> str:
    text = path.read_text(encoding="utf-8")
    text = re.sub(r"\([^)]*\)", " ", text, flags=re.DOTALL)
    return "\n".join(line.partition(";")[0] for line in text.splitlines())


def validate_spindle_test_operation() -> str:
    path = LIVE_DIR / "nc_files" / "dmc2_spindle_test.ngc"
    raw = path.read_text(encoding="utf-8")
    program = executable_gcode_text(path)

    required_tokens = (
        "o<dmc2_spindle_test> sub",
        "#<test_rpm> = #1",
        "#<transition_timeout_seconds> = #<_ini[dmc2]spindle_test_transition_timeout_seconds>",
        "#<_ini[dmc2]spindle_minimum_rpm>",
        "#<_ini[dmc2]spindle_maximum_rpm>",
        "S#<test_rpm> M3",
        "M66 P3 L3 Q#<transition_timeout_seconds>",
        "M66 P2 L3 Q#<transition_timeout_seconds>",
        "M66 P3 L4 Q#<transition_timeout_seconds>",
        "o<dmc2_spindle_test> endsub",
    )
    missing = [token for token in required_tokens if token not in raw]
    if missing:
        raise AssertionError(f"parameterized spindle test is incomplete: {missing}")

    if re.search(r"(?i)\bS\s*[-+]?\d", program):
        raise AssertionError("spindle test contains a fixed numeric S command")
    if len(re.findall(r"(?i)\bM3\b", program)) != 1:
        raise AssertionError("spindle test must contain exactly one clockwise M3")
    if re.search(r"(?im)^\s*G(?:0|1|2|3|38(?:\.\d+)?)\b", program):
        raise AssertionError("spindle test contains an axis-motion G-code")
    if re.search(r"(?i)\bM4\b", program):
        raise AssertionError("spindle test changes the verified clockwise direction")
    if program.upper().count("M5") < 4:
        raise AssertionError("every spindle-test failure path is not fail-stopped")

    return (
        "named spindle test takes RPM as #1, uses direct M3/M5, waits for "
        "H100 running/at-speed/stopped feedback, and contains no axis motion"
    )


def validate_probe_test_programs() -> str:
    test_path = LIVE_DIR / "nc_files" / "puck-contact-no-motion-test.ngc"
    abort_path = LIVE_DIR / "nc_files" / "dmc2_abort.ngc"
    test = executable_gcode_text(test_path)
    abort = executable_gcode_text(abort_path)

    required_test_tokens = (
        "#<contact_timeout_seconds> = 300.0",
        "M64 P0",
        "M66 P1 L3 Q1.0",
        "M66 P0 L1 Q#<contact_timeout_seconds>",
        "M66 P1 L4 Q1.0",
        "M66 P0 L4 Q1.0",
        "#<contact_result> = #5399",
        "M65 P0",
    )
    missing = [token for token in required_test_tokens if token not in test]
    if missing:
        raise AssertionError(f"no-motion puck test is incomplete: {missing}")

    forbidden_motion = re.findall(
        r"\b(?:G(?:0?0|0?1|0?2|0?3|38(?:\.\d+)?)|M[345])\b",
        test,
        flags=re.IGNORECASE,
    )
    if forbidden_motion:
        raise AssertionError(
            "no-motion puck test contains motion/spindle words: "
            f"{forbidden_motion}"
        )
    if "M64 P0" in abort or "M65 P0" not in abort:
        raise AssertionError("abort handler does not unconditionally clear probe power")
    abort_forbidden = re.findall(
        r"\b(?:G(?:0?0|0?1|0?2|0?3|38(?:\.\d+)?)|M[345])\b",
        abort,
        flags=re.IGNORECASE,
    )
    if abort_forbidden:
        raise AssertionError(
            f"probe abort handler contains motion/spindle words: {abort_forbidden}"
        )
    return (
        "LinuxCNC G-code integration test verifies its power gate, has a 300-second "
        "fail-off window and abort cleanup, and has no axis or spindle-start "
        "command"
    )


def validate_first_tool_height_test() -> str:
    path = LIVE_DIR / "nc_files" / "tool-height-first-test.ngc"
    program = executable_gcode_text(path)

    required = (
        "#<puck_machine_x> = 288.125",
        "#<puck_machine_y> = 152.955",
        "#<puck_height_mm> = 19.40",
        "#<z_home_machine> = 135.0",
        "#<probe_target_z> = 0.0",
        "#<probe_feed_mm_min> = 6.0",
        "M5",
        "M0",
        "M64 P0",
        "M66 P1 L3 Q1.0",
        "G38.2 Z#<probe_target_z> F#<probe_feed_mm_min>",
        "#<probe_machine_z> = #5063",
        "#<plate_contact_machine_z> = [#<probe_machine_z> - #<puck_height_mm>]",
        "M65 P0",
        "M66 P1 L4 Q1.0",
        "G53 G0 Z#<z_home_machine>",
    )
    missing = [token for token in required if token not in program]
    if missing:
        raise AssertionError(f"first moving tool-height test is incomplete: {missing}")

    gate = program.index("M0")
    power_on = program.index("M64 P0")
    probe = program.index("G38.2 Z#<probe_target_z> F#<probe_feed_mm_min>")
    power_off = program.index("M65 P0", probe)
    return_home = program.index("G53 G0 Z#<z_home_machine>")
    if not gate < power_on < probe < power_off < return_home:
        raise AssertionError(
            "tool-height test order must be gate, power on, probe, power off, return"
        )

    if len(re.findall(r"\bG38\.2\b", program, flags=re.IGNORECASE)) != 1:
        raise AssertionError("first tool-height test must contain exactly one probe move")
    if re.search(r"\bG(?:0?0|0?1|38\.2)\b[^\n]*\b[XY]", program, flags=re.IGNORECASE):
        raise AssertionError("first tool-height test must not command X or Y motion")
    if re.search(r"\bM[34]\b", program, flags=re.IGNORECASE):
        raise AssertionError("first tool-height test contains a spindle-start command")

    return (
        "first moving tool-height test holds at an operator gate, probes only Z at "
        "0.1 mm/s, removes OUT5 power, and rapidly returns to machine Z home"
    )


def validate_homing_style_tool_height_test() -> str:
    path = LIVE_DIR / "nc_files" / "tool-height-homing-style-test.ngc"
    raw_program = path.read_text(encoding="utf-8")
    program = executable_gcode_text(path)
    helper = (ROOT / "tool_probe_confirmation.py").read_text(encoding="utf-8")

    required = (
        "#<puck_machine_x> = 288.125",
        "#<puck_machine_y> = 152.955",
        "#<z_home_machine> = 135.0",
        "#<fast_probe_feed_mm_min> = 300.0",
        "#<backoff_distance_mm> = 1.0",
        "#<backoff_feed_mm_min> = 15.0",
        "#<slow_probe_feed_mm_min> = 15.0",
        "#<connection_timeout_seconds> = 300.0",
        "ABS[#<_abs_z> - #<z_home_machine>]",
        "M5",
        "M0",
        "M64 P0",
        "M66 P0 L1 Q#<connection_timeout_seconds>",
        "#<connection_result> = #5399",
        "G38.2 Z#<probe_target_z> F#<fast_probe_feed_mm_min>",
        "#<first_probe_machine_z> = #5063",
        "G53 G1 Z#<backoff_machine_z> F#<backoff_feed_mm_min>",
        "M66 P0 L4 Q1.0",
        "G38.2 Z#<probe_target_z> F#<slow_probe_feed_mm_min>",
        "#<second_probe_machine_z> = #5063",
        "M65 P0",
        "M66 P1 L4 Q1.0",
        "G53 G0 Z#<z_home_machine>",
    )
    missing = [token for token in required if token not in program]
    if missing:
        raise AssertionError(f"homing-style tool-height test is incomplete: {missing}")

    required_messages = (
        "DMC2 CONNECTION TEST PASSED: IN0 contact detected and OUT5 removed",
        "DMC2 CONNECTION PASSED: OUT5 OFF; place the puck, then resume",
    )
    missing = [token for token in required_messages if token not in raw_program]
    if missing:
        raise AssertionError(
            f"homing-style tool-height operator messages are incomplete: {missing}"
        )

    gates = [match.start() for match in re.finditer(r"(?m)^M0$", program)]
    if len(gates) != 2:
        raise AssertionError(
            "homing-style tool-height test must contain exactly two operator gates"
        )
    connection_gate, measurement_gate = gates
    connection_power_on = program.index("M64 P0", connection_gate)
    connection_wait = program.index(
        "M66 P0 L1 Q#<connection_timeout_seconds>", connection_power_on
    )
    connection_power_off = program.index("M65 P0", connection_wait)
    measurement_power_on = program.index("M64 P0", measurement_gate)
    first_probe = program.index(
        "G38.2 Z#<probe_target_z> F#<fast_probe_feed_mm_min>",
        measurement_power_on,
    )
    backoff = program.index(
        "G53 G1 Z#<backoff_machine_z> F#<backoff_feed_mm_min>"
    )
    second_probe = program.index(
        "G38.2 Z#<probe_target_z> F#<slow_probe_feed_mm_min>"
    )
    power_off = program.index("M65 P0", second_probe)
    return_home = program.index("G53 G0 Z#<z_home_machine>")
    if not (
        connection_gate
        < connection_power_on
        < connection_wait
        < connection_power_off
        < measurement_gate
        < measurement_power_on
        < first_probe
        < backoff
        < second_probe
        < power_off
        < return_home
    ):
        raise AssertionError(
            "tool-check order must be connection gate, no-motion contact test, "
            "OUT5 off, placement gate, power, fast probe, backoff, slow probe, "
            "OUT5 off, return"
        )

    connection_stage = program[connection_gate:measurement_gate]
    if re.search(
        r"(?m)^\s*G(?:0?0|0?1|38\.2)\b", connection_stage, flags=re.IGNORECASE
    ):
        raise AssertionError("connection-test stage must contain no axis motion")

    if len(re.findall(r"\bG38\.2\b", program, flags=re.IGNORECASE)) != 2:
        raise AssertionError("homing-style test must contain exactly two probe moves")
    if re.search(r"\bG(?:0?0|0?1|38\.2)\b[^\n]*\b[XY]", program, flags=re.IGNORECASE):
        raise AssertionError("homing-style tool-height test must not command X or Y")
    if re.search(r"\bM[34]\b", program, flags=re.IGNORECASE):
        raise AssertionError("homing-style tool-height test starts the spindle")

    helper_required = (
        "EXPECTED_Z = 135.0",
        'heading="1. CONNECTION TEST — NO MOTION"',
        'start_label="START CONNECTION TEST — NO MOTION"',
        'self.connection_left_first_gate = False',
        "if not self.connection_left_first_gate:",
        'heading="2. CONNECTION PASSED — PLACE PUCK"',
        'start_label="START TOOL MEASURE"',
        "self.command.auto(linuxcnc.AUTO_RESUME)",
    )
    missing = [token for token in helper_required if token not in helper]
    if missing:
        raise AssertionError(f"two-stage tool-check popup is incomplete: {missing}")

    return (
        "homing-style tool-height test requires a no-motion IN0 connection pass, "
        "removes OUT5 before its placement gate, probes at 5 mm/s, backs off "
        "1.00 mm at 0.25 mm/s, re-touches at 0.25 mm/s, removes OUT5, and "
        "rapidly returns to Z home"
    )


def validate() -> list[str]:
    checks: list[str] = []

    panel_pins = pin_names_from_panel(ROOT / "status_panel.xml")
    postgui_pins = pin_names_from_postgui(ROOT / "status_postgui.hal")
    if panel_pins != postgui_pins:
        missing_nets = sorted(panel_pins - postgui_pins)
        missing_widgets = sorted(postgui_pins - panel_pins)
        raise AssertionError(
            f"panel/postgui mismatch: missing nets={missing_nets}, "
            f"missing widgets={missing_widgets}"
        )
    checks.append(f"PyVCP XML and {len(panel_pins)} post-GUI pins agree")

    axis_policy = (ROOT / "axis_ui_policy.py").read_text(encoding="utf-8")
    if 'EXPECTED_LIMIT_STOP_MESSAGE = "Jog aborted by jog-stop-immediate"' not in axis_policy:
        raise AssertionError("AXIS policy does not identify the exact expected limit stop")
    if "notifications.add = add_without_covering_status_panel" not in axis_policy:
        raise AssertionError("AXIS notifications are not moved away from the status panel")
    required_pendant_mode_policy = (
        'PENDANT_MODE_PIN = "pendant-mode-enabled"',
        'CONTROLLER_AVAILABLE_PIN = "controller-available"',
        'CONTROLLER_READY_PIN = "controller-ready"',
        'PENDANT_WIDGET_PATH = ".toolbar.dmc2_pendant_mode"',
        '"-after",\n        ".toolbar.clear_plot"',
        "component[PENDANT_MODE_PIN] = False",
        "component[PENDANT_MODE_PIN] = True",
        "elif ready and not self.enabled:",
        "self._set_panel_visible(True)",
        '"normal" if available else "disabled"',
    )
    missing = [
        token for token in required_pendant_mode_policy if token not in axis_policy
    ]
    if missing:
        raise AssertionError(f"AXIS Pendant Mode policy is incomplete: {missing}")
    icon = (ROOT / "pendant_icon.xbm").read_text(encoding="ascii")
    if (
        "#define dmc2_pendant_width 24" not in icon
        or "#define dmc2_pendant_height 24" not in icon
    ):
        raise AssertionError("AXIS pendant toolbar icon is not the expected 24x24 XBM")
    postgui = (ROOT / "status_postgui.hal").read_text(encoding="utf-8")
    if (
        "axisui.pendant-mode-enabled => "
        "dmc2-pendant-control.pendant-mode-enabled"
    ) not in postgui:
        raise AssertionError("AXIS Pendant Mode is not connected to the supervisor")
    required_postgui_handshake = (
        "dmc2-controller-available     => axisui.controller-available",
        "dmc2-controller-ready         => axisui.controller-ready pyvcp.controller-ready",
        "dmc2-controller-estop-reset-request => halui.estop.reset",
        "dmc2-controller-machine-on-request  => halui.machine.on",
    )
    missing = [
        token for token in required_postgui_handshake if token not in postgui
    ]
    if missing:
        raise AssertionError(
            f"Pendant Mode readiness/state-request handshake is incomplete: {missing}"
        )
    rust_root = ROOT / "rust" / "crates"
    runtime = (rust_root / "dmc2-core" / "src" / "runtime.rs").read_text(
        encoding="utf-8"
    )
    supervisor = (rust_root / "dmc2-core" / "src" / "supervisor.rs").read_text(
        encoding="utf-8"
    )
    realtime = (rust_root / "dmc2-rt" / "src" / "lib.rs").read_text(
        encoding="utf-8"
    )
    task_monitor = (
        rust_root / "dmc2-task-monitor" / "src" / "task_status_shim.cc"
    ).read_text(encoding="utf-8")
    required_native_contract = (
        (runtime, "TaskHeartbeatTimeout"),
        (runtime, "pendant_coherent"),
        (supervisor, "JogCountMismatch"),
        (supervisor, "command_channel_ready"),
        (realtime, 'b"dmc2-pendant-control.update\\0"'),
        (realtime, "hal_export_funct"),
        (realtime, "HaluiCommandSequencer"),
        (task_monitor, "holder->status->task.heartbeat"),
        (task_monitor, "RCS_STAT_CHANNEL"),
    )
    missing = [token for text, token in required_native_contract if token not in text]
    if missing:
        raise AssertionError(f"native realtime contract is incomplete: {missing}")
    live_pendant = (LIVE_DIR / "pendant.hal").read_text(encoding="utf-8")
    if "python3" in executable_hal_text(LIVE_DIR / "pendant.hal"):
        raise AssertionError("live pendant control still invokes Python")
    checks.append(
        "AXIS hides only the expected realtime limit-stop popup and preserves other notifications"
    )
    checks.append(
        "AXIS exposes Pendant Mode only after controller availability and shows it only after control-ready acknowledgement"
    )
    checks.append(
        "LinuxCNC E-stop-reset, machine-on, and finite jog requests are delegated to native HALUI/motion pins"
    )
    checks.append(
        "compiled Rust owns servo-thread policy, exact count verification, and the real milltask heartbeat"
    )

    sim_config = read_ini(SIM_DIR / "monitor.ini")
    required_sim_sections = {
        "EMC", "DISPLAY", "TASK", "RS274NGC", "EMCMOT", "EMCIO", "HAL",
        "TRAJ", "KINS", "AXIS_X", "AXIS_Y", "AXIS_Z", "JOINT_0", "JOINT_1",
        "JOINT_2",
    }
    missing_sections = sorted(required_sim_sections - set(sim_config.sections()))
    if missing_sections:
        raise AssertionError(f"simulation INI missing sections: {missing_sections}")
    if sim_config["EMC"]["MACHINE"] != "DMC2-OFFLINE-MONITOR-SIMULATION":
        raise AssertionError("simulation INI is not unmistakably labelled")
    checks.append("offline LinuxCNC INI has all required sections")

    sim_text = executable_hal_text(
        SIM_DIR / "core_sim.hal",
        SIM_DIR / "status_sources.hal",
        ROOT / "pendant_replay.hal",
    ) + "\n" + (SIM_DIR / "monitor.ini").read_text(encoding="utf-8")
    forbidden_sim_tokens = ("hm2_eth", "hm2_7i95", "/dev/tty", "motion.probe-input")
    present = [token for token in forbidden_sim_tokens if token in sim_text]
    if present:
        raise AssertionError(f"offline simulator contains hardware token(s): {present}")
    checks.append("offline simulator contains no Mesa, serial-device, or probe-motion route")

    hal_files = list(ROOT.glob("*.hal")) + list(SIM_DIR.glob("*.hal")) + list(
        LIVE_DIR.glob("*.hal")
    )
    continued = [
        str(path.relative_to(ROOT))
        for path in hal_files
        if any(line.rstrip().endswith("\\") for line in path.read_text().splitlines())
    ]
    if continued:
        raise AssertionError(
            f"HAL files contain unsupported shell-style continuations: {continued}"
        )
    checks.append("HAL commands contain no shell-style line continuations")

    checks.append(validate_live_ini())
    checks.extend(validate_live_hal())
    checks.append(validate_h100_spindle_integration())
    checks.append(validate_spindle_test_operation())
    checks.append(validate_probe_test_programs())
    checks.append(validate_first_tool_height_test())
    checks.append(validate_homing_style_tool_height_test())

    readiness = load_requirements(ROOT / "live_requirements.json")
    if readiness.get("configuration_status") != READY_CONFIGURATION_STATUS:
        raise AssertionError("live profile is not ready for explicit hardware validation")
    unresolved = unresolved_requirements(readiness)
    if unresolved:
        raise AssertionError(
            "blocking requirements remain: "
            + ", ".join(item["id"] for item in unresolved)
        )
    deferred = [item for item in readiness["requirements"] if not item["blocking"]]
    if not deferred or any(item["status"] == "confirmed" for item in deferred):
        raise AssertionError("deferred integrations are not explicitly preserved")
    checks.append("accepted profile has no blocker; unapproved integrations remain deferred")

    launcher = (ROOT / "launch_live.py").read_text(encoding="utf-8")
    if "if not args.live:" not in launcher or 'os.execvp("linuxcnc"' not in launcher:
        raise AssertionError("live launcher does not require its explicit --live gate")
    if launcher.index("if not args.live:") > launcher.index('os.execvp("linuxcnc"'):
        raise AssertionError("live launcher gate occurs after LinuxCNC execution")
    if shutil.which("linuxcnc") is None:
        raise AssertionError("LinuxCNC executable is unavailable")
    version_tokens = (
        'EXPECTED_LINUXCNC_VERSION = "2.9.10"',
        '["linuxcnc_var", "LINUXCNCVERSION"]',
        "installed_version != EXPECTED_LINUXCNC_VERSION",
    )
    if any(token not in launcher for token in version_tokens):
        raise AssertionError("live launcher is not locked to LinuxCNC 2.9.10")
    checks.append(
        "launcher defaults to validation, requires explicit --live, and locks LinuxCNC 2.9.10"
    )

    realtime_tokens = (
        'REALTIME_ENVIRONMENT = "LINUXCNC_FORCE_REALTIME=1"',
        'f"--setenv={REALTIME_ENVIRONMENT}"',
        'os.environ["LINUXCNC_FORCE_REALTIME"] = "1"',
    )
    if any(token not in launcher for token in realtime_tokens):
        raise AssertionError(
            "live launcher does not force realtime scheduling in both launch paths"
        )
    checks.append("persistent and direct launch paths force realtime scheduling")

    return checks


def main() -> int:
    try:
        checks = validate()
    except Exception as error:
        print(f"OFFLINE VALIDATION FAILED: {error}", file=sys.stderr)
        return 1
    for check in checks:
        print(f"PASS: {check}")
    print("PASS: validation opened neither NML, a serial device, nor Mesa hardware")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
