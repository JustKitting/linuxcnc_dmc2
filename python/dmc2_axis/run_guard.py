"""Guard stock AXIS program-run entry points with LinuxCNC homing state."""

from __future__ import annotations

from collections.abc import Mapping

from .recovery_ui import RecoveryUiNotice
from .ui_fault import AxisUiFault, AxisUiFaultKind


class AxisRunGuard:
    """Prevent AXIS from submitting AUTO_RUN while any joint is unhomed."""

    RECOVERABLE_FAULTS = (
        AxisUiFaultKind.PROGRAM_RUN_STATUS_UNAVAILABLE,
        AxisUiFaultKind.PROGRAM_RUN_REQUIRES_HOMED_POSITION,
        AxisUiFaultKind.PROGRAM_RUN_SUBMISSION_FAILED,
    )

    def __init__(self, *, namespace, status, linuxcnc_module, stock_task_run) -> None:
        self.namespace = namespace
        self.status = status
        self.linuxcnc_module = linuxcnc_module
        self.stock_task_run = stock_task_run
        self.error_notices = {
            kind: RecoveryUiNotice(namespace) for kind in self.RECOVERABLE_FAULTS
        }
        self.active_faults: set[AxisUiFaultKind] = set()

    def _present(
        self,
        *,
        kind: AxisUiFaultKind,
        cause: object,
    ) -> None:
        route = None
        presentation_cause = cause
        try:
            reader = self.namespace["live_plotter"]._dmc2_diagnostic_reader
            route = reader.recovery_route(kind.contract.recovery_code)
        except Exception as presentation_error:
            presentation_cause = (
                f"{cause}; dynamic recovery catalog unavailable: {presentation_error}"
            )
        self.error_notices[kind].present(
            fault=AxisUiFault(
                kind,
                presentation_cause,
            ),
            route=route,
        )
        self.active_faults.add(kind)

    def _clear(self, kind: AxisUiFaultKind) -> None:
        if kind not in self.active_faults:
            return
        if self.error_notices[kind].clear():
            self.active_faults.remove(kind)

    def _status_snapshot(self) -> tuple[int, tuple[bool, ...], int]:
        self.status.poll()
        joint_count = int(self.status.joints)
        homed = tuple(bool(value) for value in self.status.homed[:joint_count])
        return joint_count, homed, int(self.status.interp_state)

    def reconcile(self) -> None:
        """Clear each retained run fault only after its typed transition occurs."""
        if not self.active_faults:
            return
        try:
            joint_count, homed, interp_state = self._status_snapshot()
        except Exception as error:
            if AxisUiFaultKind.PROGRAM_RUN_STATUS_UNAVAILABLE not in self.active_faults:
                self._present(
                    kind=AxisUiFaultKind.PROGRAM_RUN_STATUS_UNAVAILABLE,
                    cause=error,
                )
            return

        self._clear(AxisUiFaultKind.PROGRAM_RUN_STATUS_UNAVAILABLE)
        if joint_count > 0 and len(homed) == joint_count and all(homed):
            self._clear(AxisUiFaultKind.PROGRAM_RUN_REQUIRES_HOMED_POSITION)
        if interp_state == int(self.linuxcnc_module.INTERP_IDLE):
            self._clear(AxisUiFaultKind.PROGRAM_RUN_SUBMISSION_FAILED)

    def __call__(self, *args):
        try:
            joint_count, homed, _interp_state = self._status_snapshot()
        except Exception as error:
            print(
                "DMC2_AXIS_RUN_REQUEST result=blocked "
                f"reason=status-unavailable error={error!r}",
                flush=True,
            )
            self._present(
                kind=AxisUiFaultKind.PROGRAM_RUN_STATUS_UNAVAILABLE,
                cause=error,
            )
            return "break"

        if joint_count <= 0 or len(homed) != joint_count or not all(homed):
            homed_mask = sum(1 << index for index, value in enumerate(homed) if value)
            print(
                "DMC2_AXIS_RUN_REQUEST result=blocked reason=not-all-homed "
                f"joint_count={joint_count} homed_mask=0x{homed_mask:08x}",
                flush=True,
            )
            self._present(
                kind=AxisUiFaultKind.PROGRAM_RUN_REQUIRES_HOMED_POSITION,
                cause=(
                    f"joint_count={joint_count} homed_mask=0x{homed_mask:08x}"
                ),
            )
            return "break"

        print(
            "DMC2_AXIS_RUN_REQUEST result=forwarded reason=all-homed "
            f"joint_count={joint_count}",
            flush=True,
        )
        self._clear(AxisUiFaultKind.PROGRAM_RUN_STATUS_UNAVAILABLE)
        self._clear(AxisUiFaultKind.PROGRAM_RUN_REQUIRES_HOMED_POSITION)
        try:
            result = self.stock_task_run(*args)
        except Exception as error:
            self._present(
                kind=AxisUiFaultKind.PROGRAM_RUN_SUBMISSION_FAILED,
                cause=error,
            )
            return "break"
        self._clear(AxisUiFaultKind.PROGRAM_RUN_SUBMISSION_FAILED)
        return result


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
        namespace=namespace,
        status=namespace["s"],
        linuxcnc_module=namespace["linuxcnc"],
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
