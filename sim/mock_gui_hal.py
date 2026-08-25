#!/usr/bin/env python3
"""Hardware-free HAL pin mock for checking status_postgui.hal syntax/types."""

from __future__ import annotations

import signal
import time

import hal


FLOAT_PANEL_INPUTS = {
    "x-position",
    "y-position",
    "z-position",
    "x-generated-pulses",
    "y-generated-pulses",
    "z-generated-pulses",
}

S32_PANEL_INPUTS = set()

U32_PANEL_INPUTS = set()

BIT_PANEL_INPUTS = {
    "limit-x-live",
    "limit-y-live",
    "limit-z-live",
    "limit-x-latched",
    "limit-y-latched",
    "limit-z-latched",
    "puck-live",
    "probe-live",
    "puck-latched",
    "probe-latched",
    "probe-power-enabled",
    "pendant-connected",
    "pendant-link-fault",
    "pendant-quadrature-fault",
    "pendant-link-healthy",
    "pendant-estop",
    "pendant-deadman",
    "pendant-selector-valid",
    "pendant-axis-x",
    "pendant-axis-y",
    "pendant-axis-z",
    "pendant-axis-4",
    "pendant-axis-5",
    "pendant-axis-off",
    "pendant-multiplier-x1",
    "pendant-multiplier-x10",
    "pendant-multiplier-x100",
    "controller-ready",
    "controller-fault",
    "controller-recovery",
    "controller-jog-active",
    "controller-bounce-active",
}


def main() -> int:
    halui = hal.component("halui")
    for axis in "xyz":
        halui.newpin(f"axis.{axis}.pos-feedback", hal.HAL_FLOAT, hal.HAL_OUT)
    halui.newpin("home-all", hal.HAL_BIT, hal.HAL_IN)

    pyvcp = hal.component("pyvcp")
    for name in sorted(FLOAT_PANEL_INPUTS):
        pyvcp.newpin(name, hal.HAL_FLOAT, hal.HAL_IN)
    for name in sorted(S32_PANEL_INPUTS):
        pyvcp.newpin(name, hal.HAL_S32, hal.HAL_IN)
    for name in sorted(U32_PANEL_INPUTS):
        pyvcp.newpin(name, hal.HAL_U32, hal.HAL_IN)
    for name in sorted(BIT_PANEL_INPUTS):
        pyvcp.newpin(name, hal.HAL_BIT, hal.HAL_IN)
    pyvcp.newpin("clear-display-latches", hal.HAL_BIT, hal.HAL_OUT)
    pyvcp.newpin("home-all", hal.HAL_BIT, hal.HAL_OUT)

    # loadusr waits for halui, so make it ready only after the pyvcp mock is ready.
    pyvcp.ready()
    halui.ready()

    stopping = False

    def request_stop(_signum, _frame) -> None:
        nonlocal stopping
        stopping = True

    signal.signal(signal.SIGINT, request_stop)
    signal.signal(signal.SIGTERM, request_stop)
    try:
        while not stopping:
            time.sleep(0.025)
    finally:
        halui.exit()
        pyvcp.exit()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
