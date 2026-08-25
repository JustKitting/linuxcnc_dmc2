from __future__ import annotations

from collections.abc import Sequence


HalCommand = tuple[str, ...]
MOTORS = (0, 1, 2)


def guard_load_commands() -> list[HalCommand]:
    """Realtime components used by the three independent limit gates."""
    return [
        ("loadrt", "flipflop", "count=3"),
        ("loadrt", "watchdog", "num_inputs=1"),
        ("loadrt", "and2", "count=9"),
        ("loadrt", "or2", "count=6"),
        ("loadrt", "not", "count=3"),
    ]


def _net(
    signal: str,
    *,
    writer: str | None,
    readers: Sequence[str],
) -> HalCommand:
    if writer is None:
        return ("net", signal, "=>", *readers)
    if not readers:
        return ("net", signal, writer)
    return ("net", signal, writer, "=>", *readers)


def guard_net_commands(
    *,
    raw_limit_writers: Sequence[str | None],
    reset_writers: Sequence[str | None],
    toward_writers: Sequence[str | None],
    command_writers: Sequence[str | None],
    heartbeat_writer: str | None,
    watchdog_enable_writer: str | None,
    raw_limit_observers: Sequence[str | None] = (None, None, None),
    latched_observers: Sequence[str | None] = (None, None, None),
    enable_readers: Sequence[Sequence[str]] = ((), (), ()),
    watchdog_ok_observer: str | None = None,
) -> list[HalCommand]:
    """Build the exact gate used in both simulation and the live controller.

    For each motor, its own latched limit blocks only the positive/toward
    direction. Either of the other two latched limits blocks that motor in both
    directions. A realtime watchdog is in series with every command enable.
    """
    groups = (
        raw_limit_writers,
        reset_writers,
        toward_writers,
        command_writers,
        raw_limit_observers,
        latched_observers,
        enable_readers,
    )
    if any(len(group) != 3 for group in groups):
        raise ValueError("three motors require exactly three entries per pin group")

    commands: list[HalCommand] = []
    for motor in MOTORS:
        raw_readers = [f"flipflop.{motor}.set"]
        if raw_limit_observers[motor] is not None:
            raw_readers.append(raw_limit_observers[motor])
        commands.append(
            _net(
                f"pendant-limit-raw-{motor}",
                writer=raw_limit_writers[motor],
                readers=raw_readers,
            )
        )
        commands.append(
            _net(
                f"pendant-limit-reset-{motor}",
                writer=reset_writers[motor],
                readers=[f"flipflop.{motor}.reset"],
            )
        )
        commands.append(
            _net(
                f"pendant-toward-limit-{motor}",
                writer=toward_writers[motor],
                readers=[f"and2.{motor}.in1"],
            )
        )
        commands.append(
            _net(
                f"pendant-command-enable-{motor}",
                writer=command_writers[motor],
                readers=[f"and2.{3 + motor}.in0"],
            )
        )

    for motor in MOTORS:
        latched_readers = [f"and2.{motor}.in0"]
        for consumer in MOTORS:
            if consumer == motor:
                continue
            other_motors_for_consumer = tuple(
                other for other in MOTORS if other != consumer
            )
            input_index = other_motors_for_consumer.index(motor)
            latched_readers.append(f"or2.{consumer}.in{input_index}")
        if latched_observers[motor] is not None:
            latched_readers.append(latched_observers[motor])
        commands.append(
            _net(
                f"pendant-limit-latched-{motor}",
                writer=f"flipflop.{motor}.out",
                readers=latched_readers,
            )
        )
        commands.append(
            _net(
                f"pendant-own-toward-block-{motor}",
                writer=f"and2.{motor}.out",
                readers=[f"or2.{3 + motor}.in0"],
            )
        )
        commands.append(
            _net(
                f"pendant-other-limit-block-{motor}",
                writer=f"or2.{motor}.out",
                readers=[f"or2.{3 + motor}.in1"],
            )
        )
        commands.append(
            _net(
                f"pendant-blocked-{motor}",
                writer=f"or2.{3 + motor}.out",
                readers=[f"not.{motor}.in"],
            )
        )
        commands.append(
            _net(
                f"pendant-safe-{motor}",
                writer=f"not.{motor}.out",
                readers=[f"and2.{6 + motor}.in1"],
            )
        )

    watchdog_readers = [f"and2.{3 + motor}.in1" for motor in MOTORS]
    if watchdog_ok_observer is not None:
        watchdog_readers.append(watchdog_ok_observer)
    commands.extend(
        [
            _net(
                "pendant-heartbeat",
                writer=heartbeat_writer,
                readers=["watchdog.input-0"],
            ),
            _net(
                "pendant-watchdog-enable",
                writer=watchdog_enable_writer,
                readers=["watchdog.enable-in"],
            ),
            _net(
                "pendant-watchdog-ok",
                writer="watchdog.ok-out",
                readers=watchdog_readers,
            ),
        ]
    )

    for motor in MOTORS:
        commands.append(
            _net(
                f"pendant-command-watchdog-{motor}",
                writer=f"and2.{3 + motor}.out",
                readers=[f"and2.{6 + motor}.in0"],
            )
        )
        commands.append(
            _net(
                f"pendant-stepgen-enable-{motor}",
                writer=f"and2.{6 + motor}.out",
                readers=list(enable_readers[motor]),
            )
        )

    return commands


def guard_function_commands(
    *, read_function: str | None = None, write_function: str | None = None
) -> list[HalCommand]:
    commands: list[HalCommand] = []
    if read_function is not None:
        commands.append(("addf", read_function, "servo-thread"))
    for motor in MOTORS:
        commands.append(("addf", f"flipflop.{motor}", "servo-thread"))
    for motor in MOTORS:
        commands.append(("addf", f"and2.{motor}", "servo-thread"))
    for motor in MOTORS:
        commands.append(("addf", f"or2.{motor}", "servo-thread"))
    for motor in MOTORS:
        commands.append(("addf", f"or2.{3 + motor}", "servo-thread"))
    for motor in MOTORS:
        commands.append(("addf", f"not.{motor}", "servo-thread"))
    commands.extend(
        [
            ("addf", "watchdog.set-timeouts", "servo-thread"),
            ("addf", "watchdog.process", "servo-thread"),
        ]
    )
    for motor in MOTORS:
        commands.append(("addf", f"and2.{3 + motor}", "servo-thread"))
    for motor in MOTORS:
        commands.append(("addf", f"and2.{6 + motor}", "servo-thread"))
    if write_function is not None:
        commands.append(("addf", write_function, "servo-thread"))
    return commands
