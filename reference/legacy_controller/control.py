#!/usr/bin/env python3
from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[2]
PENDANT_DIR = PROJECT_ROOT / "pendant_nano"
for import_directory in (str(PENDANT_DIR), str(Path(__file__).resolve().parent)):
    if import_directory not in sys.path:
        sys.path.insert(0, import_directory)

import hal
import serial

from control_core import (
    BOUNCE_PULSES,
    BOUNCE_RATE_PULSES_PER_SECOND,
    CLOCKWISE_SIGN_BY_AXIS,
    JOG_RATE_BY_MULTIPLIER,
    JOG_RATE_PULSES_PER_SECOND,
    LIMIT_INPUT_BY_MOTOR,
    ControlFault,
    EstopRecoverySequence,
    JogRequest,
    PendantInterpreter,
    decide_limit_action,
    make_bounce_plan,
)
from hal_session import HalSession, HalSessionError
from hal_topology import (
    MOTORS,
    guard_function_commands,
    guard_load_commands,
    guard_net_commands,
)
from pendant_protocol import ProtocolError, parse_packet


COMPONENT_NAME = "pendant-cnc-control"
BOARD_HAL_NAME = "hm2_7i95.0"
WATCHDOG_TIMEOUT_SECONDS = 0.1
SERIAL_READ_TIMEOUT_SECONDS = 0.025
SERIAL_LOSS_SECONDS = 0.1
SERVO_PERIOD_NS = 1_000_000
STEP_LENGTH_NS = 5_000
STEP_SPACE_NS = 5_000
DIRECTION_SETUP_NS = 10_000
DIRECTION_HOLD_NS = 10_000


def process_is_running(name: str) -> bool:
    completed = subprocess.run(
        ["pgrep", "-x", name],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return completed.returncode == 0


def serial_port_is_busy(port: str) -> bool:
    if shutil.which("fuser") is None:
        return False
    completed = subprocess.run(
        ["fuser", port],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return completed.returncode == 0


@dataclass
class ActiveMotion:
    mode: str
    motor: int
    target_count: int
    toward_limit: bool
    start_count: int
    deadline: float | None = None


class MesaPendantHardware:
    def __init__(
        self,
        *,
        board_ip: str,
    ) -> None:
        self.board_ip = board_ip
        self.jog_rate = JOG_RATE_PULSES_PER_SECOND
        self.session = HalSession()
        self.component: hal.component | None = None

    @staticmethod
    def _stepgen(motor: int) -> str:
        return f"{BOARD_HAL_NAME}.stepgen.{motor:02d}"

    @staticmethod
    def _input(input_number: int) -> str:
        return f"{BOARD_HAL_NAME}.inmux.00.raw-input-{input_number:02d}"

    def _new_component(self) -> hal.component:
        component = hal.component(COMPONENT_NAME)
        component.newpin("heartbeat", hal.HAL_BIT, hal.HAL_OUT)
        component.newpin("watchdog-enable", hal.HAL_BIT, hal.HAL_OUT)
        component.newpin("watchdog-ok", hal.HAL_BIT, hal.HAL_IN)
        for motor in MOTORS:
            component.newpin(
                f"motor-{motor}-position-cmd", hal.HAL_FLOAT, hal.HAL_OUT
            )
            component.newpin(
                f"motor-{motor}-command-enable", hal.HAL_BIT, hal.HAL_OUT
            )
            component.newpin(
                f"motor-{motor}-toward-limit", hal.HAL_BIT, hal.HAL_OUT
            )
            component.newpin(
                f"motor-{motor}-limit-reset", hal.HAL_BIT, hal.HAL_OUT
            )
            component.newpin(f"motor-{motor}-counts", hal.HAL_S32, hal.HAL_IN)
            component.newpin(f"motor-{motor}-limit-raw", hal.HAL_BIT, hal.HAL_IN)
            component.newpin(
                f"motor-{motor}-limit-latched", hal.HAL_BIT, hal.HAL_IN
            )
            component.newpin(
                f"motor-{motor}-guard-enabled", hal.HAL_BIT, hal.HAL_IN
            )

        component["heartbeat"] = False
        component["watchdog-enable"] = False
        for motor in MOTORS:
            component[f"motor-{motor}-position-cmd"] = 0.0
            component[f"motor-{motor}-command-enable"] = False
            component[f"motor-{motor}-toward-limit"] = False
            component[f"motor-{motor}-limit-reset"] = False
        component.ready()
        return component

    def configure(self) -> None:
        self.session.start()
        self.session.commands(
            [
                (
                    "loadrt",
                    "threads",
                    "name1=servo-thread",
                    f"period1={SERVO_PERIOD_NS}",
                ),
                ("loadrt", "hostmot2"),
                (
                    "loadrt",
                    "hm2_eth",
                    f"board_ip={self.board_ip}",
                    "config=num_encoders=0 num_stepgens=3 num_pwmgens=0 "
                    "num_3pwmgens=0 num_inmuxs=1",
                ),
            ]
        )
        self.session.commands(guard_load_commands())
        self.component = self._new_component()

        self.session.commands(
            [
                ("setp", f"{BOARD_HAL_NAME}.inmux.00.scan_rate", "20000"),
                ("setp", f"{BOARD_HAL_NAME}.inmux.00.fast_scans", "0"),
            ]
        )
        for input_number in sorted(LIMIT_INPUT_BY_MOTOR.values()):
            self.session.command(
                (
                    "setp",
                    f"{BOARD_HAL_NAME}.inmux.00.input-{input_number:02d}-slow",
                    "false",
                )
            )

        for motor in MOTORS:
            stepgen = self._stepgen(motor)
            self.session.commands(
                [
                    ("setp", f"{stepgen}.control-type", "0"),
                    ("setp", f"{stepgen}.step_type", "0"),
                    ("setp", f"{stepgen}.position-scale", "1"),
                    ("setp", f"{stepgen}.maxvel", str(self.jog_rate)),
                    ("setp", f"{stepgen}.maxaccel", "0"),
                    ("setp", f"{stepgen}.steplen", str(STEP_LENGTH_NS)),
                    ("setp", f"{stepgen}.stepspace", str(STEP_SPACE_NS)),
                    ("setp", f"{stepgen}.dirsetup", str(DIRECTION_SETUP_NS)),
                    ("setp", f"{stepgen}.dirhold", str(DIRECTION_HOLD_NS)),
                    ("setp", f"{stepgen}.direction.invert_output", "false"),
                    (
                        "net",
                        f"pendant-position-command-{motor}",
                        f"{COMPONENT_NAME}.motor-{motor}-position-cmd",
                        "=>",
                        f"{stepgen}.position-cmd",
                    ),
                    (
                        "net",
                        f"pendant-generated-count-{motor}",
                        f"{stepgen}.counts",
                        "=>",
                        f"{COMPONENT_NAME}.motor-{motor}-counts",
                    ),
                ]
            )

        self.session.commands(
            guard_net_commands(
                raw_limit_writers=tuple(
                    self._input(LIMIT_INPUT_BY_MOTOR[motor]) for motor in MOTORS
                ),
                reset_writers=tuple(
                    f"{COMPONENT_NAME}.motor-{motor}-limit-reset"
                    for motor in MOTORS
                ),
                toward_writers=tuple(
                    f"{COMPONENT_NAME}.motor-{motor}-toward-limit"
                    for motor in MOTORS
                ),
                command_writers=tuple(
                    f"{COMPONENT_NAME}.motor-{motor}-command-enable"
                    for motor in MOTORS
                ),
                heartbeat_writer=f"{COMPONENT_NAME}.heartbeat",
                watchdog_enable_writer=f"{COMPONENT_NAME}.watchdog-enable",
                raw_limit_observers=tuple(
                    f"{COMPONENT_NAME}.motor-{motor}-limit-raw"
                    for motor in MOTORS
                ),
                latched_observers=tuple(
                    f"{COMPONENT_NAME}.motor-{motor}-limit-latched"
                    for motor in MOTORS
                ),
                enable_readers=tuple(
                    (
                        f"{self._stepgen(motor)}.enable",
                        f"{COMPONENT_NAME}.motor-{motor}-guard-enabled",
                    )
                    for motor in MOTORS
                ),
                watchdog_ok_observer=f"{COMPONENT_NAME}.watchdog-ok",
            )
        )
        self.session.command(
            ("setp", "watchdog.timeout-0", str(WATCHDOG_TIMEOUT_SECONDS))
        )
        self.session.commands(
            guard_function_commands(
                read_function=f"{BOARD_HAL_NAME}.read",
                write_function=f"{BOARD_HAL_NAME}.write",
            )
        )
        self.session.command(("start",))
        time.sleep(0.1)

        raw = self.raw_limits()
        if any(raw):
            active = [
                f"IN{LIMIT_INPUT_BY_MOTOR[motor]}"
                for motor, state in enumerate(raw)
                if state
            ]
            raise ControlFault(
                "startup refused because limit input is already active: "
                + ", ".join(active)
            )

        self.disable_commands()
        for motor in MOTORS:
            self.set_position(motor, self.count(motor))
            self.set_toward(motor, False)
            self.set_rate(motor, self.jog_rate)
        self.reset_limit_latches(MOTORS)
        if any(self.latched_limits()):
            raise ControlFault("startup refused because a limit latch would not clear")

    def _component(self) -> hal.component:
        if self.component is None:
            raise ControlFault("HAL userspace component is not active")
        return self.component

    def count(self, motor: int) -> int:
        return int(self._component()[f"motor-{motor}-counts"])

    def raw_limits(self) -> tuple[bool, bool, bool]:
        component = self._component()
        return tuple(bool(component[f"motor-{motor}-limit-raw"]) for motor in MOTORS)

    def latched_limits(self) -> tuple[bool, bool, bool]:
        component = self._component()
        return tuple(
            bool(component[f"motor-{motor}-limit-latched"]) for motor in MOTORS
        )

    def watchdog_ok(self) -> bool:
        return bool(self._component()["watchdog-ok"])

    def guard_enabled(self, motor: int) -> bool:
        return bool(self._component()[f"motor-{motor}-guard-enabled"])

    def set_position(self, motor: int, count: int) -> None:
        self._component()[f"motor-{motor}-position-cmd"] = float(count)

    def set_command(self, motor: int, enabled: bool) -> None:
        self._component()[f"motor-{motor}-command-enable"] = enabled

    def set_toward(self, motor: int, toward: bool) -> None:
        self._component()[f"motor-{motor}-toward-limit"] = toward

    def set_rate(self, motor: int, rate: float) -> None:
        hal.set_p(f"{self._stepgen(motor)}.maxvel", str(rate))

    def feed_heartbeat(self) -> None:
        component = self._component()
        component["heartbeat"] = not bool(component["heartbeat"])

    def start_watchdog(self) -> None:
        component = self._component()
        component["watchdog-enable"] = False
        component["heartbeat"] = False
        time.sleep(0.003)
        component["watchdog-enable"] = True

    def disable_commands(self) -> None:
        if self.component is None:
            return
        for motor in MOTORS:
            self.component[f"motor-{motor}-command-enable"] = False

    def cancel_and_align(self) -> tuple[int, int, int]:
        self.disable_commands()
        time.sleep(0.004)
        counts = tuple(self.count(motor) for motor in MOTORS)
        for motor, count in enumerate(counts):
            self.set_position(motor, count)
            self.set_toward(motor, False)
            self.set_rate(motor, self.jog_rate)
        time.sleep(0.002)
        if any(self.guard_enabled(motor) for motor in MOTORS):
            raise ControlFault("realtime gate remained enabled after command cancellation")
        return counts

    def reset_limit_latches(self, motors: tuple[int, ...] | list[int]) -> None:
        component = self._component()
        for motor in motors:
            component[f"motor-{motor}-limit-reset"] = True
        time.sleep(0.004)
        for motor in motors:
            component[f"motor-{motor}-limit-reset"] = False
        time.sleep(0.004)

    def close(self) -> None:
        component = self.component
        if component is not None:
            try:
                self.disable_commands()
                component["watchdog-enable"] = False
                time.sleep(0.01)
                for motor in MOTORS:
                    self.set_position(motor, self.count(motor))
                    self.set_toward(motor, False)
            except Exception:
                pass
            try:
                component.exit()
            except Exception:
                pass
            self.component = None
        self.session.close()


class PendantMachineController:
    def __init__(
        self,
        *,
        hardware: MesaPendantHardware,
    ) -> None:
        self.hardware = hardware
        self.interpreter = PendantInterpreter()
        self.estop_recovery = EstopRecoverySequence()
        self.active: ActiveMotion | None = None

    def cancel_normal_motion(self, reason: str) -> None:
        had_motion = self.active is not None
        self.hardware.cancel_and_align()
        self.active = None
        if had_motion:
            print(f"MOTION CANCELLED: {reason}", flush=True)

    def _begin_bounce(self, motor: int) -> None:
        self.hardware.disable_commands()
        time.sleep(0.004)
        latched = self.hardware.latched_limits()
        expected = tuple(index == motor for index in MOTORS)
        if latched != expected:
            self.hardware.cancel_and_align()
            raise ControlFault(
                f"bounce refused: expected only motor {motor} limit latch, got {latched}"
            )
        if not self.hardware.watchdog_ok():
            self.hardware.cancel_and_align()
            raise ControlFault("bounce refused because pendant watchdog is not healthy")

        counts = self.hardware.cancel_and_align()
        stopped_count = counts[motor]
        plan = make_bounce_plan(motor=motor, stopped_count=stopped_count)
        self.hardware.set_rate(motor, BOUNCE_RATE_PULSES_PER_SECOND)
        self.hardware.set_position(motor, stopped_count)
        self.hardware.set_toward(motor, False)
        self.hardware.set_position(motor, plan.target_count)
        self.hardware.set_command(motor, True)

        expected_seconds = BOUNCE_PULSES / BOUNCE_RATE_PULSES_PER_SECOND
        self.active = ActiveMotion(
            mode="bounce",
            motor=motor,
            target_count=plan.target_count,
            toward_limit=False,
            start_count=stopped_count,
            deadline=time.monotonic() + max(2.0, expected_seconds * 4.0 + 1.0),
        )
        print(
            f"LIMIT COLLISION: motor={motor} "
            f"IN{LIMIT_INPUT_BY_MOTOR[motor]} stopped_count={stopped_count}; "
            f"queue reset; bounce=-{BOUNCE_PULSES} target={plan.target_count}",
            flush=True,
        )

    def _finish_bounce(self, motion: ActiveMotion) -> None:
        motor = motion.motor
        self.hardware.set_command(motor, False)
        time.sleep(0.004)
        final_count = self.hardware.count(motor)
        actual_delta = final_count - motion.start_count
        self.hardware.set_position(motor, final_count)
        self.hardware.set_rate(motor, self.hardware.jog_rate)
        if actual_delta != -BOUNCE_PULSES:
            self.active = None
            raise ControlFault(
                f"bounce count mismatch on motor {motor}: "
                f"required -{BOUNCE_PULSES}, generated {actual_delta:+d}"
            )
        if self.hardware.raw_limits()[motor]:
            self.active = None
            raise ControlFault(
                f"motor {motor} completed exactly -{BOUNCE_PULSES} pulses "
                f"but IN{LIMIT_INPUT_BY_MOTOR[motor]} is still active"
            )
        self.hardware.reset_limit_latches([motor])
        if self.hardware.latched_limits()[motor]:
            self.active = None
            raise ControlFault(f"motor {motor} limit latch did not reset after bounce")

        self.active = None
        self.interpreter.reset()
        print(
            f"BOUNCE COMPLETE: motor={motor} generated_delta={actual_delta:+d}; "
            "limit latch reset; pendant baseline reset",
            flush=True,
        )

    def poll_motion(self) -> None:
        latched = self.hardware.latched_limits()
        motion = self.active

        if motion is None:
            if any(latched):
                self.hardware.cancel_and_align()
                raise ControlFault(
                    f"limit latched without an active commanded collision: {latched}"
                )
            return

        if motion.mode == "jog" and any(latched):
            action = decide_limit_action(
                active_motor=motion.motor,
                active_toward_limit=motion.toward_limit,
                latched_by_motor=latched,
            )
            if action.kind == "bounce" and action.motor is not None:
                self._begin_bounce(action.motor)
                return
            self.hardware.cancel_and_align()
            self.active = None
            raise ControlFault(
                f"limit event cannot be mapped to the active toward move: {latched}"
            )

        if motion.mode == "bounce":
            unexpected = tuple(
                state and motor != motion.motor
                for motor, state in enumerate(latched)
            )
            if any(unexpected):
                self.hardware.cancel_and_align()
                self.active = None
                raise ControlFault(
                    f"unrelated limit activated during motor {motion.motor} bounce: "
                    f"{latched}"
                )
            if not latched[motion.motor]:
                self.hardware.cancel_and_align()
                self.active = None
                raise ControlFault("collision latch cleared before bounce verification")

        if not self.hardware.watchdog_ok():
            self.hardware.cancel_and_align()
            self.active = None
            raise ControlFault("pendant heartbeat watchdog stopped active motion")

        current = self.hardware.count(motion.motor)
        if current == motion.target_count:
            if motion.mode == "bounce":
                self._finish_bounce(motion)
            else:
                self.hardware.set_command(motion.motor, False)
                self.hardware.set_position(motion.motor, current)
                self.active = None
            return

        crossed_target = (
            motion.toward_limit and current > motion.target_count
        ) or (not motion.toward_limit and current < motion.target_count)
        if crossed_target:
            self.hardware.cancel_and_align()
            self.active = None
            raise ControlFault(
                f"motor {motion.motor} crossed its exact count target "
                f"{motion.target_count}; observed {current}"
            )
        if motion.deadline is not None and time.monotonic() > motion.deadline:
            self.hardware.cancel_and_align()
            self.active = None
            raise ControlFault(
                f"motor {motion.motor} timed out before exact bounce target"
            )
        if not self.hardware.guard_enabled(motion.motor):
            self.hardware.cancel_and_align()
            self.active = None
            raise ControlFault(
                f"realtime gate unexpectedly disabled motor {motion.motor}"
            )

    def _start_from_rest(self, request: JogRequest) -> None:
        if any(self.hardware.raw_limits()) or any(self.hardware.latched_limits()):
            raise ControlFault("jog refused because a limit input is active or latched")
        if not self.hardware.watchdog_ok():
            raise ControlFault("jog refused because pendant watchdog is not healthy")

        motor = request.motor
        current = self.hardware.count(motor)
        target = current + request.delta_pulses
        toward = request.delta_pulses > 0
        rate = JOG_RATE_BY_MULTIPLIER[request.multiplier]
        self.hardware.set_rate(motor, rate)
        self.hardware.set_position(motor, current)
        self.hardware.set_toward(motor, toward)
        self.hardware.set_position(motor, target)
        self.hardware.set_command(motor, True)
        self.active = ActiveMotion(
            mode="jog",
            motor=motor,
            target_count=target,
            toward_limit=toward,
            start_count=current,
        )
        print(
            f"JOG axis={request.axis} motor={motor} "
            f"pulses={request.delta_pulses:+d} rate={rate:g} target={target}",
            flush=True,
        )

    def apply_jog(self, request: JogRequest) -> None:
        if request.delta_pulses == 0:
            return
        motion = self.active
        if motion is None:
            self._start_from_rest(request)
            return
        if motion.mode != "jog" or motion.motor != request.motor:
            self.cancel_normal_motion("axis changed while another move was active")
            self._start_from_rest(request)
            return

        current = self.hardware.count(motion.motor)
        rate = JOG_RATE_BY_MULTIPLIER[request.multiplier]
        self.hardware.set_rate(motion.motor, rate)
        # Latest-wins, one-slot control: replace the outstanding target with one
        # sampled increment from the current generated count. Never add another
        # request to the previous target.
        proposed_target = current + request.delta_pulses
        proposed_toward = proposed_target > current
        if proposed_toward != motion.toward_limit:
            # Disable first so the realtime direction gate never describes the
            # opposite direction while the old target can still generate steps.
            self.hardware.cancel_and_align()
            self.active = None
            self._start_from_rest(request)
            return

        self.hardware.set_position(motion.motor, proposed_target)
        motion.target_count = proposed_target
        print(
            f"JOG REPLACED axis={request.axis} motor={request.motor} "
            f"pulses={request.delta_pulses:+d} rate={rate:g} "
            f"target={proposed_target}",
            flush=True,
        )

    def process_packet(self, packet) -> None:
        if packet.estop_pressed and not self.estop_recovery.active:
            self.hardware.cancel_and_align()
            self.active = None
            self.interpreter.reset()
            update = self.estop_recovery.engage(packet)
            print(f"E-STOP RECOVERY: {update.message}", flush=True)
            return

        if self.estop_recovery.active:
            # Recovery is an input-only state. Reassert disabled commands on
            # every packet and never pass a recovery packet to normal jogging.
            self.hardware.disable_commands()
            self.active = None
            update = self.estop_recovery.process(packet)
            if update.message is not None:
                print(f"E-STOP RECOVERY: {update.message}", flush=True)
            if update.unlock_requested:
                if any(self.hardware.raw_limits()) or any(
                    self.hardware.latched_limits()
                ):
                    update = self.estop_recovery.restart(
                        packet, "a limit input is active or latched"
                    )
                    print(f"E-STOP RECOVERY: {update.message}", flush=True)
                    return
                if not self.hardware.watchdog_ok():
                    update = self.estop_recovery.restart(
                        packet, "pendant watchdog is not healthy"
                    )
                    print(f"E-STOP RECOVERY: {update.message}", flush=True)
                    return
                self.hardware.cancel_and_align()
                self.interpreter.reset()
                self.estop_recovery.accept_unlock()
                print(
                    "E-STOP RECOVERY COMPLETE: control unlocked; "
                    "fresh selector/deadman baseline required",
                    flush=True,
                )
            return

        self.poll_motion()
        decision = self.interpreter.process(packet)
        if self.active is not None and self.active.mode == "bounce":
            return
        if decision.stop:
            if self.active is not None:
                self.cancel_normal_motion(decision.reason)
            return
        if decision.jog is not None:
            self.apply_jog(decision.jog)


def open_serial_exclusive(port: str, baud: int) -> serial.Serial:
    try:
        return serial.Serial(
            port,
            baud,
            timeout=SERIAL_READ_TIMEOUT_SECONDS,
            exclusive=True,
        )
    except TypeError as error:
        raise ControlFault(
            "installed pyserial does not support exclusive port ownership"
        ) from error


def run_live(args: argparse.Namespace) -> int:
    if (
        process_is_running("linuxcnc")
        or process_is_running("halrun")
        or process_is_running("rtapi_app")
    ):
        raise ControlFault(
            "LinuxCNC, halrun, or rtapi_app is already active; no motion sent"
        )
    if serial_port_is_busy(args.port):
        raise ControlFault(f"serial port {args.port} is already in use; no motion sent")

    hardware = MesaPendantHardware(
        board_ip=args.board_ip,
    )
    connection: serial.Serial | None = None
    try:
        hardware.configure()
        connection = open_serial_exclusive(args.port, args.baud)
        controller = PendantMachineController(
            hardware=hardware,
        )
        print(
            f"LIVE CONTROL ARMED board={args.board_ip} port={args.port} "
            f"jog_speeds=X1:{JOG_RATE_BY_MULTIPLIER['X1']:g},"
            f"X10:{JOG_RATE_BY_MULTIPLIER['X10']:g},"
            f"X100:{JOG_RATE_BY_MULTIPLIER['X100']:g} "
            f"bounce={BOUNCE_PULSES}_steps_at_"
            f"{BOUNCE_RATE_PULSES_PER_SECOND:g}_steps_per_second "
            "clockwise=X-right,Y-forward,Z-up; waiting for P3 baseline",
            flush=True,
        )

        valid_packets = 0
        last_valid_at = time.monotonic()
        startup_deadline = last_valid_at + 3.0
        watchdog_started = False
        while True:
            raw = connection.readline()
            now = time.monotonic()
            if not raw:
                controller.poll_motion()
                if not watchdog_started and now >= startup_deadline:
                    raise ControlFault("no valid P3 Nano packet within 3 seconds")
                if watchdog_started and now - last_valid_at >= SERIAL_LOSS_SECONDS:
                    controller.cancel_normal_motion("Nano serial packet timeout")
                    raise ControlFault("Nano serial packet timeout")
                continue

            line = raw.decode("ascii", errors="replace").strip()
            if not line:
                continue
            if line.startswith("BOOT,"):
                controller.cancel_normal_motion("Nano rebooted")
                controller.interpreter.reset()
                if valid_packets:
                    raise ControlFault("Nano rebooted during live control")
                continue
            try:
                packet = parse_packet(line)
            except ProtocolError as error:
                controller.cancel_normal_motion("invalid Nano packet")
                raise ControlFault(f"invalid Nano protocol packet: {error}") from error

            if not watchdog_started:
                hardware.start_watchdog()
                watchdog_started = True
            hardware.feed_heartbeat()
            last_valid_at = now
            valid_packets += 1
            controller.process_packet(packet)
    finally:
        if connection is not None:
            connection.close()
        hardware.close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "MYST1474 Nano handwheel to Mesa 7I95 controller. Without --live, "
            "this only validates and prints the requested configuration."
        )
    )
    parser.add_argument("--board-ip", default="192.168.1.121")
    parser.add_argument("--port", default="/dev/ttyUSB0")
    parser.add_argument("--baud", type=int, default=115200)
    parser.add_argument(
        "--live",
        action="store_true",
        help="explicitly permit opening the Mesa board and commanding motion",
    )
    return parser


def main() -> int:
    args = build_parser().parse_args()
    if not args.live:
        print(
            "VALIDATION ONLY — no serial port, HAL realtime thread, Mesa board, "
            "or motor output was opened."
        )
        print(
            f"Configured: X1=10, X10=100, X100=1000 pulses/detent; "
            f"jog_speeds=X1:{JOG_RATE_BY_MULTIPLIER['X1']:g},"
            f"X10:{JOG_RATE_BY_MULTIPLIER['X10']:g},"
            f"X100:{JOG_RATE_BY_MULTIPLIER['X100']:g}; "
            f"bounce=-{BOUNCE_PULSES} at "
            f"{BOUNCE_RATE_PULSES_PER_SECOND:g} steps/second; "
            f"clockwise signs={CLOCKWISE_SIGN_BY_AXIS}."
        )
        return 0
    return run_live(args)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("CONTROL STOPPED: keyboard interrupt", file=sys.stderr, flush=True)
        raise SystemExit(130)
    except (ControlFault, HalSessionError, OSError, serial.SerialException) as error:
        print(f"CONTROL FAULT: {error}", file=sys.stderr, flush=True)
        raise SystemExit(1)
