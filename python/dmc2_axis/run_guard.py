"""Guard stock AXIS program-run entry points with LinuxCNC homing state."""

from __future__ import annotations

from collections.abc import Mapping


class AxisRunGuard:
    """Prevent AXIS from submitting AUTO_RUN while any joint is unhomed."""

    def __init__(self, *, status, stock_task_run) -> None:
        self.status = status
        self.stock_task_run = stock_task_run

    def __call__(self, *args):
        try:
            self.status.poll()
            joint_count = int(self.status.joints)
            homed = tuple(bool(value) for value in self.status.homed[:joint_count])
        except Exception as error:
            print(
                "DMC2_AXIS_RUN_REQUEST result=blocked "
                f"reason=status-unavailable error={error!r}",
                flush=True,
            )
            return "break"

        if joint_count <= 0 or len(homed) != joint_count or not all(homed):
            homed_mask = sum(1 << index for index, value in enumerate(homed) if value)
            print(
                "DMC2_AXIS_RUN_REQUEST result=blocked reason=not-all-homed "
                f"joint_count={joint_count} homed_mask=0x{homed_mask:08x}",
                flush=True,
            )
            return "break"

        print(
            "DMC2_AXIS_RUN_REQUEST result=forwarded reason=all-homed "
            f"joint_count={joint_count}",
            flush=True,
        )
        return self.stock_task_run(*args)


def install_axis_run_guard(namespace: Mapping[str, object]) -> AxisRunGuard:
    """Put one guard in front of every stock AXIS program-run control."""
    live_plotter = namespace["live_plotter"]
    existing = getattr(live_plotter, "_dmc2_axis_run_guard", None)
    if existing is not None:
        return existing

    root_window = namespace["root_window"]
    commands = namespace["commands"]
    stock_task_run = commands.task_run
    guard = AxisRunGuard(
        status=namespace["s"],
        stock_task_run=stock_task_run,
    )

    # AXIS installs the single-key binding before USER_COMMAND_FILE is read.
    # Replacing it prevents a newly focused AXIS window from treating an
    # unrelated lower-case "r" keystroke as permission to run a program.
    root_window.bind("r", guard)

    # AXIS's run-line and verify handlers resolve commands.task_run at call
    # time.  The toolbar uses the Tcl command registered for the original
    # method, so redirect that command through the same guard as well.
    commands.task_run = guard
    tk = root_window.tk
    stock_tcl_command = "dmc2_stock_task_run"
    if str(tk.call("info", "commands", stock_tcl_command)):
        raise RuntimeError(f"AXIS Tcl command already exists: {stock_tcl_command}")
    if not str(tk.call("info", "commands", "task_run")):
        raise RuntimeError("AXIS's stock task_run Tcl command is unavailable")
    callback_command = root_window.register(guard)
    tk.call("rename", "task_run", stock_tcl_command)
    tk.call("interp", "alias", "", "task_run", "", callback_command)

    live_plotter._dmc2_axis_run_guard = guard
    live_plotter._dmc2_stock_task_run = stock_task_run
    live_plotter._dmc2_stock_task_run_tcl = stock_tcl_command
    return guard
