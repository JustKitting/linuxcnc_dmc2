from __future__ import annotations

import contextlib
import io
import itertools
import sys
import unittest
from dataclasses import replace
from pathlib import Path


PROJECT_ROOT = Path(__file__).resolve().parents[1]
for import_directory in (
    PROJECT_ROOT / "pendant_nano",
    PROJECT_ROOT / "pendant_cnc",
    Path(__file__).resolve().parent,
):
    if str(import_directory) not in sys.path:
        sys.path.insert(0, str(import_directory))

from control_core import (
    CLOCKWISE_SIGN_BY_AXIS,
    JOG_RATE_BY_MULTIPLIER,
    MOTOR_BY_AXIS,
    PULSES_BY_MULTIPLIER,
    PendantInterpreter,
    decide_limit_action,
)
from linuxcnc_pendant_control import coherent_nano_snapshot_from_component
from live_motion_core import (
    AXIS_INDEX_BY_NAME,
    LinkSnapshot,
    LinuxCncPendantSupervisor,
    MachineSnapshot,
)
from nano_hal_bridge import (
    BOOT_MARKER,
    OVERLONG_LINE_MARKER,
    BridgeState,
    accept_line,
    publish,
)
from pendant_protocol import PendantPacket


AXES = ("X", "Y", "Z", "4", "5", "N", "I")
MULTIPLIERS = ("X1", "X10", "X100", "N", "I")
SIGNALS = (-1, 0, 1)
STATE_SPACE = tuple(
    itertools.product(
        AXES,
        MULTIPLIERS,
        (False, True),
        (False, True),
        (False, True),
        SIGNALS,
    )
)


READY_TELEOP = MachineSnapshot(
    machine_on=True,
    estopped=False,
    manual_mode=True,
    joint_mode=False,
    teleop_mode=True,
    interp_idle=True,
    homed=(True, True, True),
    homing=(False, False, False),
    axis_stopped=(True, True, True),
)
READY_JOINT = replace(
    READY_TELEOP,
    joint_mode=True,
    teleop_mode=False,
    homed=(False, False, False),
)


def packet(
    sequence: int,
    *,
    axis: str,
    multiplier: str,
    deadman: bool,
    estop: bool = False,
    valid: bool = True,
    signal: int = 0,
) -> PendantPacket:
    return PendantPacket(
        sequence=sequence & 0xFFFFFFFF,
        milliseconds=(sequence * 20) & 0xFFFFFFFF,
        detent_count=signal,
        transition_count=signal * 4,
        quadrature_errors=0,
        latest_detent_signal=signal,
        axis=axis,
        multiplier=multiplier,
        deadman_held=deadman,
        estop_pressed=estop,
        selector_valid=valid,
    )


def packet_line(sample: PendantPacket) -> str:
    return (
        f"P3,{sample.sequence},{sample.milliseconds},{sample.detent_count},"
        f"{sample.transition_count},{sample.quadrature_errors},"
        f"{sample.latest_detent_signal},{sample.axis},{sample.multiplier},"
        f"{int(sample.deadman_held)},{int(sample.estop_pressed)},"
        f"{int(sample.selector_valid)}"
    )


def should_command(state) -> bool:
    axis, multiplier, deadman, estop, valid, signal = state
    return (
        axis in MOTOR_BY_AXIS
        and multiplier in PULSES_BY_MULTIPLIER
        and deadman
        and not estop
        and valid
        and signal in (-1, 1)
    )


def expected_command(state, *, joint_jog: bool):
    axis, multiplier, _deadman, _estop, _valid, signal = state
    delta = (
        signal
        * CLOCKWISE_SIGN_BY_AXIS[axis]
        * PULSES_BY_MULTIPLIER[multiplier]
    )
    velocity = JOG_RATE_BY_MULTIPLIER[multiplier] / 1000.0
    if delta < 0:
        velocity = -velocity
    return (
        "jog",
        joint_jog,
        AXIS_INDEX_BY_NAME[axis],
        velocity,
        abs(delta) / 1000.0,
    )


class RecordingBackend:
    def __init__(self):
        self.commands = []

    def stop_axis(self, axis_index: int, *, joint_jog: bool) -> None:
        self.commands.append(("stop", joint_jog, axis_index))

    def abort(self) -> None:
        self.commands.append(("abort",))

    def prepare_manual_teleop(self) -> None:
        self.commands.append(("manual-teleop",))

    def jog_increment(
        self,
        axis_index: int,
        signed_velocity: float,
        distance: float,
        *,
        joint_jog: bool,
    ) -> None:
        self.commands.append(
            ("jog", joint_jog, axis_index, signed_velocity, distance)
        )

    def request_estop_reset(self) -> None:
        self.commands.append(("estop-reset",))

    def request_machine_on(self) -> None:
        self.commands.append(("machine-on",))

    def clear_state_requests(self) -> None:
        pass


def arm_selection_for(state):
    axis, multiplier, *_rest = state
    return (
        axis if axis in MOTOR_BY_AXIS else "X",
        multiplier if multiplier in PULSES_BY_MULTIPLIER else "X1",
    )


class ExhaustivePendantPasses(unittest.TestCase):
    def test_state_space_is_exactly_840_packets(self):
        self.assertEqual(len(STATE_SPACE), 840)
        self.assertEqual(sum(map(should_command, STATE_SPACE)), 18)

    def test_pass_1_all_840_states_through_direct_interpreter(self):
        command_count = 0
        for state in STATE_SPACE:
            with self.subTest(state=state):
                axis, multiplier, deadman, estop, valid, signal = state
                arm_axis, arm_multiplier = arm_selection_for(state)
                interpreter = PendantInterpreter()
                interpreter.process(
                    packet(
                        1,
                        axis=arm_axis,
                        multiplier=arm_multiplier,
                        deadman=False,
                    )
                )
                interpreter.process(
                    packet(
                        2,
                        axis=arm_axis,
                        multiplier=arm_multiplier,
                        deadman=True,
                    )
                )
                decision = interpreter.process(
                    packet(
                        3,
                        axis=axis,
                        multiplier=multiplier,
                        deadman=deadman,
                        estop=estop,
                        valid=valid,
                        signal=signal,
                    )
                )

                if should_command(state):
                    command_count += 1
                    expected_delta = (
                        signal
                        * CLOCKWISE_SIGN_BY_AXIS[axis]
                        * PULSES_BY_MULTIPLIER[multiplier]
                    )
                    self.assertFalse(decision.stop)
                    self.assertIsNotNone(decision.jog)
                    self.assertEqual(decision.jog.motor, MOTOR_BY_AXIS[axis])
                    self.assertEqual(decision.jog.delta_pulses, expected_delta)
                    self.assertEqual(decision.jog.axis, axis)
                    self.assertEqual(decision.jog.multiplier, multiplier)
                else:
                    self.assertIsNone(decision.jog)

        self.assertEqual(command_count, 18)

    def test_pass_2_all_840_raw_states_through_both_linuxcnc_jog_modes(self):
        for machine, joint_jog in ((READY_TELEOP, False), (READY_JOINT, True)):
            command_count = 0
            for state in STATE_SPACE:
                with self.subTest(joint_jog=joint_jog, state=state):
                    axis, multiplier, deadman, estop, valid, signal = state
                    arm_axis, arm_multiplier = arm_selection_for(state)
                    bridge = BridgeState(0.100)
                    backend = RecordingBackend()
                    supervisor = LinuxCncPendantSupervisor(
                        backend,
                        pulses_per_mm=1000,
                    )
                    supervisor._message = lambda _message: None
                    component = {}

                    samples = (
                        packet(
                            1,
                            axis=arm_axis,
                            multiplier=arm_multiplier,
                            deadman=False,
                        ),
                        packet(
                            2,
                            axis=arm_axis,
                            multiplier=arm_multiplier,
                            deadman=True,
                        ),
                        packet(
                            3,
                            axis=axis,
                            multiplier=multiplier,
                            deadman=deadman,
                            estop=estop,
                            valid=valid,
                            signal=signal,
                        ),
                    )
                    for index, raw_sample in enumerate(samples):
                        now = index * 0.020
                        accept_line(bridge, packet_line(raw_sample), now)
                        snapshot = bridge.snapshot
                        publish(component, snapshot, bridge.packet_age_ms(now))
                        coherent = coherent_nano_snapshot_from_component(component)
                        self.assertIsNotNone(coherent)
                        supervisor.update(
                            now=now,
                            link=LinkSnapshot(
                                connected=coherent.connected,
                                serial_fault=coherent.serial_fault,
                                quadrature_fault=coherent.quadrature_fault,
                                estop_pressed=coherent.packet.estop_pressed,
                            ),
                            packet=coherent.packet,
                            machine=machine,
                            counts_by_motor=(1000, 2000, 3000),
                            raw_limits=(False, False, False),
                            safety_limits=(False, False, False),
                        )

                    jogs = [item for item in backend.commands if item[0] == "jog"]
                    if should_command(state):
                        command_count += 1
                        self.assertEqual(
                            jogs,
                            [expected_command(state, joint_jog=joint_jog)],
                        )
                        self.assertIsNotNone(supervisor.active)
                        self.assertEqual(
                            supervisor.active.request.motor,
                            MOTOR_BY_AXIS[axis],
                        )
                    else:
                        self.assertEqual(jogs, [])

            self.assertEqual(command_count, 18)


class ExhaustiveMachineAndLimitPasses(unittest.TestCase):
    def test_pass_1_all_machine_readiness_boolean_states(self):
        for values in itertools.product((False, True), repeat=8):
            (
                machine_on,
                estopped,
                manual,
                joint_mode,
                teleop_mode,
                idle,
                all_homed,
                any_homing,
            ) = values
            machine = MachineSnapshot(
                machine_on=machine_on,
                estopped=estopped,
                manual_mode=manual,
                joint_mode=joint_mode,
                teleop_mode=teleop_mode,
                interp_idle=idle,
                homed=(all_homed,) * 3,
                homing=(any_homing, False, False),
                axis_stopped=(True, True, True),
            )
            expected = (
                machine_on
                and not estopped
                and manual
                and (teleop_mode if all_homed else joint_mode)
                and idle
                and not any_homing
            )
            self.assertEqual(machine.ready_for_pendant_jog, expected)

    def test_pass_1_complete_limit_action_truth_table(self):
        for active_motor, toward, limits in itertools.product(
            (None, 0, 1, 2),
            (False, True),
            itertools.product((False, True), repeat=3),
        ):
            limits = tuple(limits)
            action = decide_limit_action(
                active_motor=active_motor,
                active_toward_limit=toward,
                latched_by_motor=limits,
            )
            active = [index for index, value in enumerate(limits) if value]
            if not active:
                expected = "none"
            elif (
                len(active) == 1
                and active_motor is not None
                and active == [active_motor]
                and toward
            ):
                expected = "bounce"
            else:
                expected = "fault"
            self.assertEqual(action.kind, expected)

    def test_pass_2_every_motor_direction_and_limit_vector_in_supervisor(self):
        axis_by_motor = {0: "Y", 1: "X", 2: "Z"}
        positive_signal_by_axis = {"X": -1, "Y": 1, "Z": 1}
        for motor, toward, limits in itertools.product(
            range(3),
            (False, True),
            itertools.product((False, True), repeat=3),
        ):
            limits = tuple(limits)
            axis = axis_by_motor[motor]
            signal = positive_signal_by_axis[axis] * (1 if toward else -1)
            backend = RecordingBackend()
            supervisor = LinuxCncPendantSupervisor(backend, pulses_per_mm=1000)
            supervisor._message = lambda _message: None
            machine = replace(
                READY_TELEOP,
                axis_stopped=(False, False, False),
            )
            link = LinkSnapshot(True, False, False, False)
            samples = (
                packet(1, axis=axis, multiplier="X1", deadman=False),
                packet(2, axis=axis, multiplier="X1", deadman=True),
                packet(
                    3,
                    axis=axis,
                    multiplier="X1",
                    deadman=True,
                    signal=signal,
                ),
            )
            for index, sample in enumerate(samples):
                supervisor.update(
                    now=index * 0.020,
                    link=link,
                    packet=sample,
                    machine=machine,
                    counts_by_motor=(1000, 2000, 3000),
                    raw_limits=(False, False, False),
                    safety_limits=(False, False, False),
                )
            supervisor.update(
                now=0.060,
                link=link,
                packet=packet(4, axis=axis, multiplier="X1", deadman=True),
                machine=machine,
                counts_by_motor=(1000, 2000, 3000),
                raw_limits=limits,
                safety_limits=limits,
            )

            active = [index for index, value in enumerate(limits) if value]
            if not active:
                self.assertFalse(supervisor.faulted)
                self.assertFalse(supervisor.bounce_active)
            elif toward and active == [motor]:
                self.assertFalse(supervisor.faulted)
                self.assertTrue(supervisor.bounce_active)
                self.assertEqual(supervisor.collision_motor, motor)
            else:
                self.assertTrue(supervisor.faulted)
                self.assertFalse(supervisor.bounce_active)


class SustainedLoadPasses(unittest.TestCase):
    def test_pass_1_one_hundred_thousand_packets_never_create_a_count_queue(self):
        interpreter = PendantInterpreter()
        interpreter.process(packet(1, axis="X", multiplier="X100", deadman=False))
        interpreter.process(packet(2, axis="X", multiplier="X100", deadman=True))
        for sequence in range(3, 100_003):
            decision = interpreter.process(
                packet(
                    sequence,
                    axis="X",
                    multiplier="X100",
                    deadman=True,
                    signal=1,
                )
            )
            self.assertEqual(decision.jog.delta_pulses, -1000)

    def test_pass_2_one_hundred_thousand_reversals_keep_one_pending_slot(self):
        backend = RecordingBackend()
        supervisor = LinuxCncPendantSupervisor(backend, pulses_per_mm=1000)
        supervisor._message = lambda _message: None
        machine = replace(READY_TELEOP, axis_stopped=(False, False, False))
        link = LinkSnapshot(True, False, False, False)
        supervisor.update(
            now=0.000,
            link=link,
            packet=packet(1, axis="X", multiplier="X1", deadman=False),
            machine=machine,
            counts_by_motor=(1000, 2000, 3000),
            raw_limits=(False, False, False),
            safety_limits=(False, False, False),
        )
        supervisor.update(
            now=0.020,
            link=link,
            packet=packet(2, axis="X", multiplier="X1", deadman=True),
            machine=machine,
            counts_by_motor=(1000, 2000, 3000),
            raw_limits=(False, False, False),
            safety_limits=(False, False, False),
        )
        for offset in range(100_000):
            supervisor.update(
                now=0.040 + offset * 0.001,
                link=link,
                packet=packet(
                    3 + offset,
                    axis="X",
                    multiplier="X1",
                    deadman=True,
                    signal=1 if offset % 2 == 0 else -1,
                ),
                machine=machine,
                counts_by_motor=(1000, 2000, 3000),
                raw_limits=(False, False, False),
                safety_limits=(False, False, False),
            )

        self.assertFalse(supervisor.faulted)
        self.assertIsNotNone(supervisor.active)
        self.assertIsNotNone(supervisor.pending)
        self.assertEqual(
            sum(command[0] == "jog" for command in backend.commands),
            1,
        )
        self.assertEqual(
            sum(command[0] == "stop" for command in backend.commands),
            1,
        )


class LinkLifecyclePasses(unittest.TestCase):
    def test_pass_1_bridge_lifecycle_is_fail_closed_and_wrap_safe(self):
        state = BridgeState(0.100)
        accept_line(state, BOOT_MARKER, 0.000)
        self.assertFalse(state.snapshot.connected)
        self.assertTrue(state.snapshot.serial_fault)

        first = packet(
            1,
            axis="X",
            multiplier="X1",
            deadman=True,
            signal=1,
        )
        accept_line(state, packet_line(first), 0.020)
        self.assertTrue(state.snapshot.connected)
        self.assertEqual(state.snapshot.latest_detent, 0)

        gap = packet(
            50,
            axis="X",
            multiplier="X1",
            deadman=True,
            signal=1,
        )
        accept_line(state, packet_line(gap), 0.040)
        self.assertEqual(state.snapshot.dropped_packets, 48)
        self.assertEqual(state.snapshot.latest_detent, 1)

        accept_line(state, packet_line(gap), 0.060)
        self.assertFalse(state.snapshot.connected)
        self.assertTrue(state.snapshot.serial_fault)
        self.assertEqual(state.snapshot.latest_detent, 0)

        reconnect = packet(
            0,
            axis="X",
            multiplier="X1",
            deadman=True,
            signal=-1,
        )
        accept_line(state, packet_line(reconnect), 0.080)
        self.assertEqual(state.snapshot.latest_detent, 0)
        self.assertTrue(state.check_timeout(0.181))
        self.assertFalse(state.snapshot.connected)

        wrap = BridgeState(0.100)
        accept_line(
            wrap,
            packet_line(
                packet(
                    0xFFFFFFFF,
                    axis="Z",
                    multiplier="X100",
                    deadman=True,
                )
            ),
            1.000,
        )
        accept_line(
            wrap,
            packet_line(
                packet(
                    0,
                    axis="Z",
                    multiplier="X100",
                    deadman=True,
                    signal=-1,
                )
            ),
            1.020,
        )
        self.assertTrue(wrap.snapshot.connected)
        self.assertEqual(wrap.snapshot.latest_detent, -1)

        for malformed in (
            "P2,1,20,0,0,0,0,X,X1,1,0,1",
            "P3,too,few,fields",
            OVERLONG_LINE_MARKER,
            "P3,4294967296,20,0,0,0,0,X,X1,1,0,1",
        ):
            with self.subTest(malformed=malformed):
                invalid = BridgeState(0.100)
                accept_line(invalid, malformed, 2.000)
                self.assertFalse(invalid.snapshot.connected)
                self.assertTrue(invalid.snapshot.serial_fault)
                self.assertEqual(invalid.snapshot.latest_detent, 0)

    def test_pass_2_gap_is_one_command_and_each_link_fault_stops_pipeline(self):
        fault_lines = (
            BOOT_MARKER,
            OVERLONG_LINE_MARKER,
            "P3,2,40,0,0,0,1,X,X1,1,0,1",
        )
        for fault_line in fault_lines:
            with self.subTest(fault_line=fault_line):
                bridge = BridgeState(0.100)
                backend = RecordingBackend()
                supervisor = LinuxCncPendantSupervisor(backend, pulses_per_mm=1000)
                supervisor._message = lambda _message: None
                component = {}
                for index, sample in enumerate(
                    (
                        packet(1, axis="X", multiplier="X1", deadman=False),
                        packet(2, axis="X", multiplier="X1", deadman=True),
                        packet(
                            50,
                            axis="X",
                            multiplier="X1",
                            deadman=True,
                            signal=1,
                        ),
                    )
                ):
                    now = index * 0.020
                    accept_line(bridge, packet_line(sample), now)
                    publish(component, bridge.snapshot, 0.0)
                    coherent = coherent_nano_snapshot_from_component(component)
                    self.assertIsNotNone(coherent)
                    supervisor.update(
                        now=now,
                        link=LinkSnapshot(
                            coherent.connected,
                            coherent.serial_fault,
                            coherent.quadrature_fault,
                            coherent.packet.estop_pressed,
                        ),
                        packet=coherent.packet,
                        machine=READY_TELEOP,
                        counts_by_motor=(1000, 2000, 3000),
                        raw_limits=(False, False, False),
                        safety_limits=(False, False, False),
                    )

                self.assertEqual(bridge.snapshot.dropped_packets, 47)
                self.assertEqual(
                    sum(command[0] == "jog" for command in backend.commands),
                    1,
                )

                accept_line(bridge, fault_line, 0.080)
                publish(component, bridge.snapshot, 0.0)
                coherent = coherent_nano_snapshot_from_component(component)
                self.assertIsNotNone(coherent)
                supervisor.update(
                    now=0.080,
                    link=LinkSnapshot(
                        coherent.connected,
                        coherent.serial_fault,
                        coherent.quadrature_fault,
                        coherent.packet.estop_pressed,
                    ),
                    packet=None,
                    machine=READY_TELEOP,
                    counts_by_motor=(1000, 2000, 3000),
                    raw_limits=(False, False, False),
                    safety_limits=(False, False, False),
                )
                self.assertTrue(supervisor.faulted)
                self.assertEqual(
                    sum(command[0] == "jog" for command in backend.commands),
                    1,
                )


if __name__ == "__main__":
    with contextlib.redirect_stdout(io.StringIO()):
        unittest.main()
