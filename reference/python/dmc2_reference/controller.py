#!/usr/bin/env python3
"""Supervise the Nano pendant through LinuxCNC's command interface.

LinuxCNC remains the only owner of the Mesa 7I95T. This process reads HAL
status produced by ``nano_hal_bridge.py`` and requests finite LinuxCNC axis
jogs. It never loads HostMot2 or directly controls a Mesa motion/output pin.
During startup only, it shares HostMot2's bidirectional watchdog status signal
so a stale bite from the preceding controller session is cleared before any
input is interpreted.
"""

from __future__ import annotations

import argparse
import json
import signal
import sys
import time
from dataclasses import dataclass
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[3]
for import_directory in (
    PROJECT_ROOT / "firmware" / "pendant_nano",
    PROJECT_ROOT / "reference" / "legacy_controller",
    Path(__file__).resolve().parent,
):
    if str(import_directory) not in sys.path:
        sys.path.insert(0, str(import_directory))

from pendant_protocol import PendantPacket

from control_core import (
    BOUNCE_PULSES,
    BOUNCE_RATE_PULSES_PER_SECOND,
    JOG_RATE_BY_MULTIPLIER,
)
from .watchdog import (
    PHASE_ARM_LOW as CONTROLLER_WATCHDOG_ARM_LOW,
    PHASE_FAULT as CONTROLLER_WATCHDOG_FAULT,
    PHASE_READY as CONTROLLER_WATCHDOG_READY,
    PHASE_STABLE as CONTROLLER_WATCHDOG_STABLE,
    PHASE_WAIT_PREREQUISITES as CONTROLLER_WATCHDOG_WAIT_PREREQUISITES,
    PHASE_WAIT_OK as CONTROLLER_WATCHDOG_WAIT_OK,
    ControllerWatchdogStartupGuard,
    HeartbeatGenerator,
)
from .supervisor import (
    AXIS_INDEX_BY_NAME,
    LinkSnapshot,
    LinuxCncPendantSupervisor,
    MachineSnapshot,
)
from .mesa_startup import (
    PHASE_FAULT as MESA_PHASE_FAULT,
    PHASE_LIMIT_RESET as MESA_PHASE_LIMIT_RESET,
    PHASE_LIMIT_SETTLE as MESA_PHASE_LIMIT_SETTLE,
    PHASE_READY as MESA_PHASE_READY,
    PHASE_WAIT_SERVO as MESA_PHASE_WAIT_SERVO,
    PHASE_WATCHDOG_STABLE as MESA_PHASE_WATCHDOG_STABLE,
    MesaStartupGuard,
)


COMPONENT_NAME = "dmc2-pendant-control"
AXIS_BY_CODE = {-2: "I", -1: "N", 0: "X", 1: "Y", 2: "Z", 3: "4", 4: "5"}
MULTIPLIER_BY_CODE = {-1: "I", 0: "N", 1: "X1", 10: "X10", 100: "X100"}
LOOP_PERIOD_SECONDS = 0.001
STOPPED_VELOCITY_EPSILON = 0.000_001
COHERENT_SNAPSHOT_ATTEMPTS = 8


@dataclass(frozen=True)
class NanoHalSnapshot:
    connected: bool
    serial_fault: bool
    quadrature_fault: bool
    packet: PendantPacket


class LinuxCncBackend:
    def __init__(self, component) -> None:
        import linuxcnc

        self.linuxcnc = linuxcnc
        self.command = linuxcnc.command()
        self.status = linuxcnc.stat()
        self.component = component
        self._pending_jogs: list[tuple[int, float, float, bool]] = []

    def snapshot(self) -> MachineSnapshot:
        lc = self.linuxcnc
        self.status.poll()
        joints = tuple(self.status.joint[index] for index in range(3))
        axes = tuple(self.status.axis[index] for index in range(3))

        def axis_stopped(index: int) -> bool:
            joint = joints[index]
            axis = axes[index]
            velocity = float(axis.get("velocity", joint.get("velocity", 0.0)))
            return bool(joint.get("inpos", self.status.inpos)) and (
                abs(velocity) <= STOPPED_VELOCITY_EPSILON
            )

        return MachineSnapshot(
            machine_on=bool(self.status.enabled),
            estopped=bool(self.status.estop),
            manual_mode=self.status.task_mode == lc.MODE_MANUAL,
            joint_mode=self.status.motion_mode == lc.TRAJ_MODE_FREE,
            teleop_mode=self.status.motion_mode == lc.TRAJ_MODE_TELEOP,
            interp_idle=self.status.interp_state == lc.INTERP_IDLE,
            homed=tuple(bool(joint.get("homed", False)) for joint in joints),
            homing=tuple(bool(joint.get("homing", False)) for joint in joints),
            axis_stopped=tuple(axis_stopped(index) for index in range(3)),
        )

    def stop_axis(self, axis_index: int, *, joint_jog: bool) -> None:
        self.command.jog(self.linuxcnc.JOG_STOP, joint_jog, axis_index)

    def abort(self) -> None:
        self.command.abort()

    def prepare_manual_teleop(self) -> None:
        lc = self.linuxcnc
        self.status.poll()
        if self.status.task_mode != lc.MODE_MANUAL:
            self.command.mode(lc.MODE_MANUAL)
            self.command.wait_complete(0.5)
        self.status.poll()
        if self.status.motion_mode != lc.TRAJ_MODE_TELEOP:
            self.command.teleop_enable(1)
            self.command.wait_complete(0.5)

    def jog_increment(
        self,
        axis_index: int,
        signed_velocity: float,
        distance: float,
        *,
        joint_jog: bool,
    ) -> None:
        # The realtime command-enable/direction pins must be published before
        # LinuxCNC can consume the matching NML jog.  Queue only this motion
        # command; the main loop flushes it immediately after publishing HAL.
        self._pending_jogs.append(
            (axis_index, signed_velocity, distance, joint_jog)
        )

    def flush_pending_jogs(self) -> None:
        pending = self._pending_jogs
        self._pending_jogs = []
        for axis_index, signed_velocity, distance, joint_jog in pending:
            self.command.jog(
                self.linuxcnc.JOG_INCREMENT,
                joint_jog,
                axis_index,
                signed_velocity,
                distance,
            )

    def discard_pending_jogs(self) -> None:
        self._pending_jogs = []

    def request_estop_reset(self) -> None:
        # Keep every potentially blocking LinuxCNC state command out of this
        # process: this same loop supplies the 100 ms safety heartbeat.  HALUI
        # owns the NML request and this output stays asserted until status
        # acknowledges the reset or the supervisor clears/re-arms the request.
        self.component["machine-on-request"] = False
        self.component["estop-reset-request"] = True

    def request_machine_on(self) -> None:
        self.component["estop-reset-request"] = False
        self.component["machine-on-request"] = True

    def clear_state_requests(self) -> None:
        self.component["estop-reset-request"] = False
        self.component["machine-on-request"] = False


INPUT_PINS = (
    ("pendant-mode-enabled", "bit"),
    ("ui-ready", "bit"),
    ("software-watchdog-ok", "bit"),
    ("snapshot-generation", "u32"),
    ("connected", "bit"),
    ("serial-fault", "bit"),
    ("quadrature-fault", "bit"),
    ("estop-pressed", "bit"),
    ("deadman-held", "bit"),
    ("selector-valid", "bit"),
    ("axis-code", "s32"),
    ("multiplier-code", "s32"),
    ("latest-detent", "s32"),
    ("detent-count", "s32"),
    ("transition-count", "s32"),
    ("quadrature-errors", "u32"),
    ("sequence", "u32"),
    ("milliseconds", "u32"),
    ("motor-0-count", "s32"),
    ("motor-1-count", "s32"),
    ("motor-2-count", "s32"),
    ("motor-0-position-feedback", "float"),
    ("motor-1-position-feedback", "float"),
    ("motor-2-position-feedback", "float"),
    ("motor-0-limit-raw", "bit"),
    ("motor-1-limit-raw", "bit"),
    ("motor-2-limit-raw", "bit"),
    ("motor-0-limit-latched", "bit"),
    ("motor-1-limit-latched", "bit"),
    ("motor-2-limit-latched", "bit"),
    ("servo-thread-ready", "bit"),
    ("mesa-packet-error", "bit"),
    ("mesa-packet-error-total", "u32"),
    ("mesa-packet-error-exceeded", "bit"),
)

OUTPUT_PINS = (
    ("external-enable", "bit"),
    ("watchdog-enable", "bit"),
    ("heartbeat", "bit"),
    ("position-known", "bit"),
    ("position-unknown", "bit"),
    ("control-available", "bit"),
    ("control-ready", "bit"),
    ("estop-reset-request", "bit"),
    ("machine-on-request", "bit"),
    ("fault", "bit"),
    ("recovery-active", "bit"),
    ("jog-active", "bit"),
    ("bounce-active", "bit"),
    ("active-axis", "s32"),
    ("phase-code", "s32"),
    ("motor-0-limit-reset", "bit"),
    ("motor-1-limit-reset", "bit"),
    ("motor-2-limit-reset", "bit"),
    ("motor-0-command-enable", "bit"),
    ("motor-1-command-enable", "bit"),
    ("motor-2-command-enable", "bit"),
    ("motor-0-toward-limit", "bit"),
    ("motor-1-toward-limit", "bit"),
    ("motor-2-toward-limit", "bit"),
)

PHASE_CODES = {
    "idle": 0,
    "stopping-cancel": 1,
    "stopping-replace": 2,
    "stopping-bounce": 3,
    "bouncing": 4,
    "bounce-reset": 5,
    "startup-gate-settle": 6,
    "startup-wait-reset": 7,
    "startup-wait-on": 8,
    MESA_PHASE_WAIT_SERVO: 9,
    MESA_PHASE_WATCHDOG_STABLE: 10,
    MESA_PHASE_LIMIT_RESET: 11,
    MESA_PHASE_LIMIT_SETTLE: 12,
    MESA_PHASE_READY: 13,
    MESA_PHASE_FAULT: 14,
    "startup-ready-gate-settle": 15,
    "startup-ready-wait-reset": 16,
    "startup-ready-wait-on": 17,
    CONTROLLER_WATCHDOG_WAIT_PREREQUISITES: 18,
    CONTROLLER_WATCHDOG_ARM_LOW: 19,
    CONTROLLER_WATCHDOG_WAIT_OK: 20,
    CONTROLLER_WATCHDOG_STABLE: 21,
    CONTROLLER_WATCHDOG_READY: 22,
    CONTROLLER_WATCHDOG_FAULT: 23,
}


def create_hal_component(component_name: str):
    import hal

    type_by_name = {
        "bit": hal.HAL_BIT,
        "float": hal.HAL_FLOAT,
        "s32": hal.HAL_S32,
        "u32": hal.HAL_U32,
    }
    component = hal.component(component_name)
    component.newpin("mesa-watchdog-has-bit", hal.HAL_BIT, hal.HAL_IO)
    for pin_name, type_name in INPUT_PINS:
        component.newpin(pin_name, type_by_name[type_name], hal.HAL_IN)
    for pin_name, type_name in OUTPUT_PINS:
        component.newpin(pin_name, type_by_name[type_name], hal.HAL_OUT)
    component["external-enable"] = False
    component["watchdog-enable"] = False
    component["heartbeat"] = False
    component["position-known"] = False
    component["position-unknown"] = True
    component["control-available"] = False
    component["control-ready"] = False
    component["estop-reset-request"] = False
    component["machine-on-request"] = False
    component["fault"] = False
    component["recovery-active"] = False
    component["jog-active"] = False
    component["bounce-active"] = False
    component["active-axis"] = -1
    component["phase-code"] = 0
    for motor in range(3):
        component[f"motor-{motor}-limit-reset"] = False
        component[f"motor-{motor}-command-enable"] = False
        component[f"motor-{motor}-toward-limit"] = False
    component.ready()
    return component


def publish_startup_guard(component, guard: MesaStartupGuard) -> None:
    component["external-enable"] = False
    component["watchdog-enable"] = False
    component["control-available"] = False
    component["control-ready"] = False
    component["estop-reset-request"] = False
    component["machine-on-request"] = False
    component["fault"] = guard.faulted
    component["recovery-active"] = not guard.ready and not guard.faulted
    component["jog-active"] = False
    component["bounce-active"] = False
    component["active-axis"] = -1
    component["phase-code"] = PHASE_CODES[guard.phase]
    for motor in range(3):
        component[f"motor-{motor}-limit-reset"] = guard.limit_reset[motor]
        component[f"motor-{motor}-command-enable"] = False
        component[f"motor-{motor}-toward-limit"] = False


def publish_controller_watchdog_guard(
    component,
    guard: ControllerWatchdogStartupGuard,
) -> None:
    component["external-enable"] = False
    component["watchdog-enable"] = guard.enable
    component["control-available"] = False
    component["control-ready"] = False
    component["estop-reset-request"] = False
    component["machine-on-request"] = False
    component["fault"] = guard.faulted
    component["recovery-active"] = not guard.ready and not guard.faulted
    component["jog-active"] = False
    component["bounce-active"] = False
    component["active-axis"] = -1
    component["phase-code"] = PHASE_CODES[guard.phase]
    for motor in range(3):
        component[f"motor-{motor}-limit-reset"] = False
        component[f"motor-{motor}-command-enable"] = False
        component[f"motor-{motor}-toward-limit"] = False


def packet_from_component(component) -> PendantPacket:
    axis_code = int(component["axis-code"])
    multiplier_code = int(component["multiplier-code"])
    if axis_code not in AXIS_BY_CODE:
        axis_code = -2
    if multiplier_code not in MULTIPLIER_BY_CODE:
        multiplier_code = -1
    latest_detent = int(component["latest-detent"])
    if latest_detent not in (-1, 0, 1):
        latest_detent = 0
    return PendantPacket(
        sequence=int(component["sequence"]) & 0xFFFFFFFF,
        milliseconds=int(component["milliseconds"]) & 0xFFFFFFFF,
        detent_count=int(component["detent-count"]),
        transition_count=int(component["transition-count"]),
        quadrature_errors=int(component["quadrature-errors"]) & 0xFFFFFFFF,
        latest_detent_signal=latest_detent,
        axis=AXIS_BY_CODE[axis_code],
        multiplier=MULTIPLIER_BY_CODE[multiplier_code],
        deadman_held=bool(component["deadman-held"]),
        estop_pressed=bool(component["estop-pressed"]),
        selector_valid=bool(component["selector-valid"]),
    )


def coherent_nano_snapshot_from_component(
    component,
    *,
    attempts: int = COHERENT_SNAPSHOT_ATTEMPTS,
) -> NanoHalSnapshot | None:
    """Read one complete bridge publication or return without using it."""
    if attempts <= 0:
        raise ValueError("coherent snapshot attempts must be positive")
    for _attempt in range(attempts):
        generation_before = int(component["snapshot-generation"]) & 0xFFFFFFFF
        if generation_before & 1:
            continue
        connected = bool(component["connected"])
        serial_fault = bool(component["serial-fault"])
        quadrature_fault = bool(component["quadrature-fault"])
        packet = packet_from_component(component)
        generation_after = int(component["snapshot-generation"]) & 0xFFFFFFFF
        if generation_before == generation_after and not generation_after & 1:
            return NanoHalSnapshot(
                connected=connected,
                serial_fault=serial_fault,
                quadrature_fault=quadrature_fault,
                packet=packet,
            )
    return None


def publish_supervisor(component, supervisor: LinuxCncPendantSupervisor) -> None:
    component["external-enable"] = supervisor.external_enable
    component["control-available"] = supervisor.control_available
    component["control-ready"] = supervisor.control_ready
    component["fault"] = supervisor.faulted
    component["recovery-active"] = supervisor.recovery_active
    component["jog-active"] = supervisor.active is not None
    component["bounce-active"] = supervisor.bounce_active
    component["active-axis"] = (
        supervisor.active.axis_index if supervisor.active is not None else -1
    )
    component["phase-code"] = PHASE_CODES.get(supervisor.phase, -1)
    command_enabled = supervisor.command_enabled_by_motor
    toward_limit = supervisor.toward_limit_by_motor
    for motor in range(3):
        component[f"motor-{motor}-limit-reset"] = supervisor.limit_reset[motor]
        component[f"motor-{motor}-command-enable"] = command_enabled[motor]
        component[f"motor-{motor}-toward-limit"] = toward_limit[motor]


def publish_position_validity(component, machine: MachineSnapshot) -> None:
    position_known = machine.all_homed
    component["position-known"] = position_known
    component["position-unknown"] = not position_known


def validate_only(pulses_per_mm: int) -> int:
    result = {
        "mode": "validation-only",
        "mesa_opened": False,
        "serial_opened": False,
        "linuxcnc_commanded": False,
        "pulses_per_mm": pulses_per_mm,
        "bounce_pulses": BOUNCE_PULSES,
        "bounce_rate_pulses_per_second": BOUNCE_RATE_PULSES_PER_SECOND,
        "jog_rates_pulses_per_second": JOG_RATE_BY_MULTIPLIER,
        "clockwise_axis_signs": {"X": -1, "Y": 1, "Z": 1},
    }
    print(json.dumps(result, sort_keys=True))
    return 0


def run_component(args: argparse.Namespace) -> int:
    component = create_hal_component(args.component)
    backend = LinuxCncBackend(component)
    startup_guard = MesaStartupGuard()
    controller_watchdog_guard = ControllerWatchdogStartupGuard()
    heartbeat = HeartbeatGenerator()
    supervisor = LinuxCncPendantSupervisor(
        backend,
        pulses_per_mm=args.pulses_per_mm,
        packet_timeout_seconds=args.packet_timeout_ms / 1000.0,
    )
    stopping = False
    last_sequence: int | None = None
    backend_missing_since: float | None = None
    last_startup_phase: str | None = None
    last_controller_watchdog_phase: str | None = None
    last_packet_error_total: int | None = None
    nano_snapshot: NanoHalSnapshot | None = None

    def request_stop(_signum, _frame) -> None:
        nonlocal stopping
        stopping = True

    signal.signal(signal.SIGINT, request_stop)
    signal.signal(signal.SIGTERM, request_stop)

    safe_machine = MachineSnapshot(
        machine_on=False,
        estopped=True,
        manual_mode=False,
        joint_mode=True,
        teleop_mode=False,
        interp_idle=False,
        homed=(False, False, False),
        homing=(False, False, False),
        axis_stopped=(True, True, True),
    )

    try:
        while not stopping:
            now = time.monotonic()
            component["heartbeat"] = heartbeat.update(now)
            component["watchdog-enable"] = controller_watchdog_guard.enable
            packet_error_total = int(component["mesa-packet-error-total"])
            if last_packet_error_total is None:
                last_packet_error_total = packet_error_total
            elif packet_error_total != last_packet_error_total:
                print(
                    "DMC2 MESA: transient packet-error-total changed "
                    f"{last_packet_error_total} -> {packet_error_total}; "
                    "stale-feedback guard active for reported bad cycles",
                    flush=True,
                )
                last_packet_error_total = packet_error_total
            try:
                machine = backend.snapshot()
                backend_missing_since = None
            except Exception as error:
                machine = safe_machine
                if backend_missing_since is None:
                    backend_missing_since = now
                elif (
                    controller_watchdog_guard.runtime_committed
                    and now - backend_missing_since > 3.0
                ):
                    supervisor.fail(f"LinuxCNC status unavailable: {error}")

            if supervisor.faulted:
                publish_position_validity(component, machine)
                publish_supervisor(component, supervisor)
                backend.discard_pending_jogs()
                time.sleep(LOOP_PERIOD_SECONDS)
                continue

            startup_guard.update(
                now=now,
                servo_thread_ready=bool(component["servo-thread-ready"]),
                watchdog_has_bit=bool(component["mesa-watchdog-has-bit"]),
                io_error=bool(component["mesa-packet-error-exceeded"]),
            )
            if startup_guard.watchdog_clear_requested:
                component["mesa-watchdog-has-bit"] = False

            if startup_guard.phase != last_startup_phase:
                startup_messages = {
                    MESA_PHASE_WAIT_SERVO: "waiting for Mesa servo thread",
                    MESA_PHASE_WATCHDOG_STABLE: (
                        "automatically clearing and validating Mesa watchdog"
                    ),
                    MESA_PHASE_LIMIT_RESET: "clearing stale startup limit latches",
                    MESA_PHASE_LIMIT_SETTLE: "settling live limit inputs",
                    MESA_PHASE_READY: "Mesa watchdog and limit inputs are ready",
                    MESA_PHASE_FAULT: f"FAULT — {startup_guard.fault}",
                }
                print(
                    f"DMC2 STARTUP: {startup_messages[startup_guard.phase]}",
                    flush=True,
                )
                last_startup_phase = startup_guard.phase

            if startup_guard.faulted:
                supervisor.fail(startup_guard.fault or "Mesa startup failed")
                publish_position_validity(component, machine)
                publish_supervisor(component, supervisor)
                backend.discard_pending_jogs()
                time.sleep(LOOP_PERIOD_SECONDS)
                continue

            if not startup_guard.ready:
                publish_position_validity(component, machine)
                publish_startup_guard(component, startup_guard)
                backend.discard_pending_jogs()
                time.sleep(LOOP_PERIOD_SECONDS)
                continue

            coherent_snapshot = coherent_nano_snapshot_from_component(component)
            if coherent_snapshot is not None:
                nano_snapshot = coherent_snapshot
            startup_prerequisites_ready = (
                bool(component["ui-ready"])
                and backend_missing_since is None
                and nano_snapshot is not None
                and nano_snapshot.connected
                and not nano_snapshot.serial_fault
                and not nano_snapshot.quadrature_fault
            )

            watchdog_was_ready = controller_watchdog_guard.ready
            controller_watchdog_guard.update(
                now=now,
                watchdog_ok=bool(component["software-watchdog-ok"]),
                prerequisites_ready=startup_prerequisites_ready,
            )
            if (
                watchdog_was_ready
                and not controller_watchdog_guard.ready
                and not controller_watchdog_guard.faulted
            ):
                supervisor.restart_uncommitted_startup(now=now)
            component["watchdog-enable"] = controller_watchdog_guard.enable
            if controller_watchdog_guard.phase != last_controller_watchdog_phase:
                watchdog_messages = {
                    CONTROLLER_WATCHDOG_WAIT_PREREQUISITES: (
                        "waiting for AXIS, LinuxCNC status, and Nano startup "
                        "prerequisites"
                    ),
                    CONTROLLER_WATCHDOG_ARM_LOW: (
                        "holding controller heartbeat watchdog low for clean re-arm"
                    ),
                    CONTROLLER_WATCHDOG_WAIT_OK: (
                        "arming controller heartbeat watchdog"
                    ),
                    CONTROLLER_WATCHDOG_STABLE: (
                        "validating controller heartbeat watchdog stability"
                    ),
                    CONTROLLER_WATCHDOG_READY: (
                        "controller heartbeat watchdog is stable"
                    ),
                    CONTROLLER_WATCHDOG_FAULT: (
                        f"FAULT — {controller_watchdog_guard.fault}"
                    ),
                }
                print(
                    "DMC2 STARTUP: "
                    f"{watchdog_messages[controller_watchdog_guard.phase]}",
                    flush=True,
                )
                last_controller_watchdog_phase = controller_watchdog_guard.phase

            if controller_watchdog_guard.faulted:
                supervisor.fail(
                    controller_watchdog_guard.fault
                    or "controller heartbeat watchdog failed"
                )
                publish_position_validity(component, machine)
                publish_supervisor(component, supervisor)
                backend.discard_pending_jogs()
                time.sleep(LOOP_PERIOD_SECONDS)
                continue

            if not controller_watchdog_guard.ready:
                publish_position_validity(component, machine)
                publish_controller_watchdog_guard(component, controller_watchdog_guard)
                backend.discard_pending_jogs()
                time.sleep(LOOP_PERIOD_SECONDS)
                continue

            packet = None
            if nano_snapshot is None:
                connected = False
                serial_fault = True
                quadrature_fault = False
                estop_pressed = True
            else:
                connected = nano_snapshot.connected
                serial_fault = nano_snapshot.serial_fault
                quadrature_fault = nano_snapshot.quadrature_fault
                estop_pressed = nano_snapshot.packet.estop_pressed
                current_sequence = nano_snapshot.packet.sequence
                if (
                    connected
                    and not serial_fault
                    and current_sequence != last_sequence
                ):
                    packet = nano_snapshot.packet
                    last_sequence = current_sequence

            link = LinkSnapshot(
                connected=connected,
                serial_fault=serial_fault,
                quadrature_fault=quadrature_fault,
                estop_pressed=estop_pressed,
            )
            counts = tuple(
                int(component[f"motor-{motor}-count"]) for motor in range(3)
            )
            position_feedback = tuple(
                float(component[f"motor-{motor}-position-feedback"])
                for motor in range(3)
            )
            raw_limits = tuple(
                bool(component[f"motor-{motor}-limit-raw"])
                for motor in range(3)
            )
            safety_limits = tuple(
                bool(component[f"motor-{motor}-limit-latched"])
                for motor in range(3)
            )

            try:
                supervisor.update(
                    now=now,
                    link=link,
                    packet=packet,
                    machine=machine,
                    counts_by_motor=counts,
                    position_feedback_by_motor=position_feedback,
                    raw_limits=raw_limits,
                    safety_limits=safety_limits,
                    pendant_mode_enabled=bool(component["pendant-mode-enabled"]),
                )
                if (
                    supervisor.startup_reset_complete
                    and not controller_watchdog_guard.runtime_committed
                ):
                    controller_watchdog_guard.commit_runtime()
                publish_position_validity(component, machine)
                publish_supervisor(component, supervisor)
                backend.flush_pending_jogs()
            except Exception as error:
                backend.discard_pending_jogs()
                supervisor.fail(f"LinuxCNC command adapter failed: {error}")
                publish_position_validity(component, machine)
                publish_supervisor(component, supervisor)
            time.sleep(LOOP_PERIOD_SECONDS)
        return 0
    finally:
        backend.discard_pending_jogs()
        try:
            for axis_index in AXIS_INDEX_BY_NAME.values():
                backend.stop_axis(axis_index, joint_jog=machine.joint_mode)
            backend.abort()
        except Exception:
            pass
        component["external-enable"] = False
        component["watchdog-enable"] = False
        component["position-known"] = False
        component["position-unknown"] = True
        component["control-available"] = False
        component["control-ready"] = False
        backend.clear_state_requests()
        component["fault"] = True
        for motor in range(3):
            component[f"motor-{motor}-command-enable"] = False
            component[f"motor-{motor}-toward-limit"] = False
        component.exit()


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--component", default=COMPONENT_NAME)
    parser.add_argument("--pulses-per-mm", type=int, default=1000)
    parser.add_argument("--packet-timeout-ms", type=float, default=100.0)
    parser.add_argument("--validate", action="store_true")
    args = parser.parse_args(argv)
    if args.pulses_per_mm <= 0:
        parser.error("--pulses-per-mm must be positive")
    if args.packet_timeout_ms <= 0:
        parser.error("packet timeout must be positive")
    return args


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if args.validate:
        return validate_only(args.pulses_per_mm)
    print(
        "LEGACY CONTROLLER DISABLED: live authority moved to the compiled "
        "dmc2_rt LinuxCNC component.",
        file=sys.stderr,
    )
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
