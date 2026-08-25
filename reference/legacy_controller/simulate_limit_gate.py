#!/usr/bin/env python3
from __future__ import annotations

import time

import hal

from hal_session import HalSession
from hal_topology import (
    guard_function_commands,
    guard_load_commands,
    guard_net_commands,
)


def wait_for(pin: str, expected: bool, *, timeout: float = 0.25) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if bool(hal.get_value(pin)) is expected:
            return
        time.sleep(0.002)
    actual = bool(hal.get_value(pin))
    raise AssertionError(f"{pin}: expected {expected}, got {actual}")


def set_signal(session: HalSession, signal: str, value: bool) -> None:
    session.command(("sets", signal, "true" if value else "false"))


def pulse_reset(session: HalSession, motor: int) -> None:
    signal = f"pendant-limit-reset-{motor}"
    set_signal(session, signal, True)
    time.sleep(0.005)
    set_signal(session, signal, False)
    time.sleep(0.005)


def arm_watchdog(session: HalSession) -> None:
    set_signal(session, "pendant-watchdog-enable", False)
    set_signal(session, "pendant-heartbeat", False)
    time.sleep(0.005)
    set_signal(session, "pendant-watchdog-enable", True)
    for heartbeat in (True, False, True):
        time.sleep(0.005)
        set_signal(session, "pendant-heartbeat", heartbeat)


def main() -> int:
    with HalSession() as session:
        observer = hal.component("pendant-gate-sim-observer")
        for motor in range(3):
            observer.newpin(f"limit-latched-{motor}", hal.HAL_BIT, hal.HAL_IN)
            observer.newpin(f"stepgen-enable-{motor}", hal.HAL_BIT, hal.HAL_IN)
        observer.newpin("watchdog-ok", hal.HAL_BIT, hal.HAL_IN)
        observer.ready()
        try:
            session.command(
                ("loadrt", "threads", "name1=servo-thread", "period1=1000000")
            )
            session.commands(guard_load_commands())
            session.commands(
                guard_net_commands(
                    raw_limit_writers=(None, None, None),
                    reset_writers=(None, None, None),
                    toward_writers=(None, None, None),
                    command_writers=(None, None, None),
                    heartbeat_writer=None,
                    watchdog_enable_writer=None,
                    latched_observers=tuple(
                        f"pendant-gate-sim-observer.limit-latched-{motor}"
                        for motor in range(3)
                    ),
                    enable_readers=tuple(
                        (f"pendant-gate-sim-observer.stepgen-enable-{motor}",)
                        for motor in range(3)
                    ),
                    watchdog_ok_observer="pendant-gate-sim-observer.watchdog-ok",
                )
            )
            session.command(("setp", "watchdog.timeout-0", "0.2"))
            session.commands(guard_function_commands())
            session.command(("start",))

            arm_watchdog(session)
            try:
                wait_for("pendant-gate-sim-observer.watchdog-ok", True)
            except AssertionError as error:
                states = {
                    name: hal.get_value(name)
                    for name in (
                        "watchdog.enable-in",
                        "watchdog.input-0",
                        "watchdog.ok-out",
                        "watchdog.timeout-0",
                        "pendant-gate-sim-observer.watchdog-ok",
                    )
                }
                raise AssertionError(
                    f"{error}; watchdog states={states}; "
                    f"HAL transcript={session.transcript!r}"
                ) from error

            set_signal(session, "pendant-command-enable-0", True)
            set_signal(session, "pendant-toward-limit-0", True)
            wait_for("pendant-gate-sim-observer.stepgen-enable-0", True)

            # Matching limit: realtime stops toward motion and latches it.
            set_signal(session, "pendant-limit-raw-0", True)
            wait_for("pendant-gate-sim-observer.limit-latched-0", True)
            wait_for("pendant-gate-sim-observer.stepgen-enable-0", False)

            # Its own latch permits only the away/bounce direction.
            set_signal(session, "pendant-limit-raw-0", False)
            set_signal(session, "pendant-toward-limit-0", False)
            wait_for("pendant-gate-sim-observer.stepgen-enable-0", True)

            # Any unrelated limit still blocks the bounce.
            set_signal(session, "pendant-limit-raw-1", True)
            wait_for("pendant-gate-sim-observer.limit-latched-1", True)
            wait_for("pendant-gate-sim-observer.stepgen-enable-0", False)
            set_signal(session, "pendant-limit-raw-1", False)
            pulse_reset(session, 1)
            wait_for("pendant-gate-sim-observer.stepgen-enable-0", True)

            # Loss of pendant heartbeat disables every axis in realtime.
            time.sleep(0.25)
            wait_for("pendant-gate-sim-observer.watchdog-ok", False)
            wait_for("pendant-gate-sim-observer.stepgen-enable-0", False)
        finally:
            observer.exit()

    print(
        "SIMULATION PASS: toward blocked; own-limit away permitted; "
        "other-limit blocked; watchdog blocked"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
