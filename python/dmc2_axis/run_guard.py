"""Guard stock AXIS Run and Step using the selected script's typed contract."""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from enum import Enum

from .recovery_ui import RecoveryUiNotice
from .script_contract import (
    ScriptPrerequisite,
    same_machine_file,
)
from .ui_fault import AxisUiFault, AxisUiFaultKind


class ExecutionRequest(Enum):
    RUN = "task_run"
    STEP = "task_step"


@dataclass(frozen=True)
class RunStatus:
    joint_count: int
    homed: tuple[bool, ...]
    interpreter_state: int
    task_state: int
    task_mode: int
    estop: bool
    enabled: bool
    loaded_file: object

    @property
    def all_homed(self) -> bool:
        return (
            self.joint_count > 0
            and len(self.homed) == self.joint_count
            and all(self.homed)
        )


class AxisRunGuard:
    """Forward an explicit Run only after its declared state is observed."""

    RECOVERABLE_FAULTS = (
        AxisUiFaultKind.PROGRAM_RUN_STATUS_UNAVAILABLE,
        AxisUiFaultKind.PROGRAM_RUN_REQUIRES_EXACT_LOADED_FILE,
        AxisUiFaultKind.PROGRAM_RUN_REQUIRES_MACHINE_READY,
        AxisUiFaultKind.PROGRAM_RUN_REQUIRES_IDLE_INTERPRETER,
        AxisUiFaultKind.PROGRAM_RUN_REQUIRES_HOMED_POSITION,
        AxisUiFaultKind.PROGRAM_RUN_SUBMISSION_FAILED,
        AxisUiFaultKind.PROGRAM_RUN_GUARD_EVALUATION_FAILED,
    )

    def __init__(self, *, namespace, status, linuxcnc_module, stock_commands) -> None:
        self.namespace = namespace
        self.status = status
        self.linuxcnc_module = linuxcnc_module
        self.stock_commands = stock_commands
        self.error_notices = {
            kind: RecoveryUiNotice(namespace) for kind in self.RECOVERABLE_FAULTS
        }
        self.active_faults: set[AxisUiFaultKind] = set()

    def _present(self, *, kind: AxisUiFaultKind, cause: object) -> None:
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
            fault=AxisUiFault(kind, presentation_cause),
            route=route,
        )
        self.active_faults.add(kind)

    def _clear(self, kind: AxisUiFaultKind) -> None:
        if kind not in self.active_faults:
            return
        if self.error_notices[kind].clear():
            self.active_faults.remove(kind)

    def _status_snapshot(self) -> RunStatus:
        self.status.poll()
        joint_count = int(self.status.joints)
        return RunStatus(
            joint_count=joint_count,
            homed=tuple(bool(value) for value in self.status.homed[:joint_count]),
            interpreter_state=int(self.status.interp_state),
            task_state=int(self.status.task_state),
            task_mode=int(self.status.task_mode),
            estop=bool(self.status.estop),
            enabled=bool(self.status.enabled),
            loaded_file=self.status.file,
        )

    def _prerequisites(
        self, requested_path: object, *, refresh: bool = False
    ) -> tuple[tuple[ScriptPrerequisite, ...], str] | None:
        loader = getattr(
            self.namespace["live_plotter"], "_dmc2_axis_script_loader", None
        )
        if loader is not None:
            contract = (
                loader.inspect_for_run(requested_path)
                if refresh
                else loader.contract_for_loaded_path(requested_path)
            )
            if contract is not None:
                return contract.prerequisites, contract.source.value
            if refresh:
                return None
        if loader is None:
            raise RuntimeError(
                "Script loader is unavailable; Run and Step remain blocked. "
                "Use the independent recovery controls and relaunch the matching application."
            )
        # No selected contract means no prerequisites can be established or
        # used to clear a retained fault. File Open establishes the contract.
        return None

    def _machine_requirements_satisfied(
        self,
        prerequisites: tuple[ScriptPrerequisite, ...],
        snapshot: RunStatus,
    ) -> bool:
        estop_clear = (
            ScriptPrerequisite.ESTOP_CLEAR not in prerequisites
            or (
                snapshot.task_state != int(self.linuxcnc_module.STATE_ESTOP)
                and not snapshot.estop
            )
        )
        machine_on = (
            ScriptPrerequisite.MACHINE_ON not in prerequisites
            or (
                snapshot.task_state == int(self.linuxcnc_module.STATE_ON)
                and snapshot.enabled
            )
        )
        return estop_clear and machine_on

    def reconcile(self) -> None:
        """Clear retained run faults only after their typed transitions occur."""
        if not self.active_faults:
            return
        try:
            snapshot = self._status_snapshot()
        except Exception as error:
            if AxisUiFaultKind.PROGRAM_RUN_STATUS_UNAVAILABLE not in self.active_faults:
                self._present(
                    kind=AxisUiFaultKind.PROGRAM_RUN_STATUS_UNAVAILABLE,
                    cause=error,
                )
            return

        self._clear(AxisUiFaultKind.PROGRAM_RUN_STATUS_UNAVAILABLE)
        requested_path = self.namespace.get("loaded_file")
        if same_machine_file(requested_path, snapshot.loaded_file):
            self._clear(AxisUiFaultKind.PROGRAM_RUN_REQUIRES_EXACT_LOADED_FILE)
        contract = self._prerequisites(requested_path)
        if contract is None:
            return
        prerequisites, _source = contract
        if self._machine_requirements_satisfied(prerequisites, snapshot):
            self._clear(AxisUiFaultKind.PROGRAM_RUN_REQUIRES_MACHINE_READY)
        if snapshot.interpreter_state == int(self.linuxcnc_module.INTERP_IDLE):
            self._clear(AxisUiFaultKind.PROGRAM_RUN_REQUIRES_IDLE_INTERPRETER)
            self._clear(AxisUiFaultKind.PROGRAM_RUN_SUBMISSION_FAILED)
        if (
            ScriptPrerequisite.ALL_HOMED not in prerequisites
            or snapshot.all_homed
        ):
            self._clear(AxisUiFaultKind.PROGRAM_RUN_REQUIRES_HOMED_POSITION)

    def _submit_if_allowed(self, request: ExecutionRequest, *args):
        try:
            snapshot = self._status_snapshot()
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

        self._clear(AxisUiFaultKind.PROGRAM_RUN_STATUS_UNAVAILABLE)
        requested_path = self.namespace.get("loaded_file")
        if not same_machine_file(requested_path, snapshot.loaded_file):
            print(
                "DMC2_AXIS_RUN_REQUEST result=blocked reason=exact-file-not-loaded "
                f"axis_selected={requested_path!r} linuxcnc_loaded={snapshot.loaded_file!r}",
                flush=True,
            )
            self._present(
                kind=AxisUiFaultKind.PROGRAM_RUN_REQUIRES_EXACT_LOADED_FILE,
                cause=(
                    f"axis_selected={requested_path!r} "
                    f"linuxcnc_loaded={snapshot.loaded_file!r}"
                ),
            )
            return "break"
        self._clear(AxisUiFaultKind.PROGRAM_RUN_REQUIRES_EXACT_LOADED_FILE)

        contract = self._prerequisites(requested_path, refresh=True)
        if contract is None:
            print(
                "DMC2_AXIS_RUN_REQUEST result=blocked "
                "reason=script-contract-inspection-failed",
                flush=True,
            )
            return "break"
        prerequisites, contract_source = contract
        prerequisite_names = ";".join(value.value for value in prerequisites)
        if not self._machine_requirements_satisfied(prerequisites, snapshot):
            print(
                "DMC2_AXIS_RUN_REQUEST result=blocked reason=machine-not-ready "
                f"task_state={snapshot.task_state} estop={snapshot.estop} "
                f"enabled={snapshot.enabled} prerequisites={prerequisite_names!r}",
                flush=True,
            )
            self._present(
                kind=AxisUiFaultKind.PROGRAM_RUN_REQUIRES_MACHINE_READY,
                cause=(
                    f"task_state={snapshot.task_state} estop={snapshot.estop} "
                    f"enabled={snapshot.enabled} prerequisites={prerequisite_names!r}"
                ),
            )
            return "break"
        self._clear(AxisUiFaultKind.PROGRAM_RUN_REQUIRES_MACHINE_READY)

        if (
            ScriptPrerequisite.INTERPRETER_IDLE in prerequisites
            and snapshot.interpreter_state != int(self.linuxcnc_module.INTERP_IDLE)
            # An active AUTO Step continues the already loaded program. Idle
            # is a start prerequisite, not a requirement to abandon stepping.
            and not (
                request is ExecutionRequest.STEP
                and snapshot.task_mode == int(self.linuxcnc_module.MODE_AUTO)
            )
        ):
            print(
                "DMC2_AXIS_RUN_REQUEST result=blocked reason=interpreter-not-idle "
                f"interpreter_state={snapshot.interpreter_state}",
                flush=True,
            )
            self._present(
                kind=AxisUiFaultKind.PROGRAM_RUN_REQUIRES_IDLE_INTERPRETER,
                cause=f"interpreter_state={snapshot.interpreter_state}",
            )
            return "break"
        self._clear(AxisUiFaultKind.PROGRAM_RUN_REQUIRES_IDLE_INTERPRETER)

        if (
            ScriptPrerequisite.ALL_HOMED in prerequisites
            and not snapshot.all_homed
        ):
            homed_mask = sum(
                1 << index for index, value in enumerate(snapshot.homed) if value
            )
            print(
                "DMC2_AXIS_RUN_REQUEST result=blocked reason=not-all-homed "
                f"joint_count={snapshot.joint_count} homed_mask=0x{homed_mask:08x}",
                flush=True,
            )
            self._present(
                kind=AxisUiFaultKind.PROGRAM_RUN_REQUIRES_HOMED_POSITION,
                cause=(
                    f"joint_count={snapshot.joint_count} "
                    f"homed_mask=0x{homed_mask:08x}"
                ),
            )
            return "break"
        self._clear(AxisUiFaultKind.PROGRAM_RUN_REQUIRES_HOMED_POSITION)

        print(
            "DMC2_AXIS_RUN_REQUEST result=forwarded "
            f"control={request.value!r} "
            f"contract_source={contract_source!r} "
            f"prerequisites={prerequisite_names!r} file={requested_path!r}",
            flush=True,
        )
        try:
            result = self.stock_commands[request.value](*args)
        except Exception as error:
            self._present(
                kind=AxisUiFaultKind.PROGRAM_RUN_SUBMISSION_FAILED,
                cause=error,
            )
            return "break"
        self._clear(AxisUiFaultKind.PROGRAM_RUN_SUBMISSION_FAILED)
        return result

    def submit(self, command_name: str, *args):
        try:
            return self._submit_if_allowed(ExecutionRequest(command_name), *args)
        except Exception as error:
            print(
                "DMC2_AXIS_RUN_REQUEST result=blocked "
                f"reason=guard-evaluation-failed error={error!r}",
                flush=True,
            )
            try:
                self._present(
                    kind=AxisUiFaultKind.PROGRAM_RUN_GUARD_EVALUATION_FAILED,
                    cause=error,
                )
            except Exception as presentation_error:
                print(
                    "DMC2_AXIS_RUN_GUARD_PRESENTATION_FAILED "
                    f"cause={presentation_error!r} original={error!r}; "
                    "ui-path=machine.abort -> controller.clear-fault -> "
                    "controller.pendant-mode",
                    flush=True,
                )
            return "break"

    def __call__(self, *args):
        return self.submit(ExecutionRequest.RUN.value, *args)


def install_axis_run_guard(namespace: Mapping[str, object]) -> AxisRunGuard:
    """Activate the early closed Run/Step boundary only after dependencies load."""
    live_plotter = namespace["live_plotter"]
    existing = getattr(live_plotter, "_dmc2_axis_run_guard", None)
    if existing is not None:
        return existing

    if namespace.get("_dmc2_execution_interlock_errors"):
        raise RuntimeError("Run/Step interlock installation failed; execution remains blocked")
    if getattr(live_plotter, "_dmc2_axis_script_loader", None) is None:
        raise RuntimeError("Script loader installation failed; Run and Step remain blocked")
    guard = AxisRunGuard(
        namespace=namespace,
        status=namespace["s"],
        linuxcnc_module=namespace["linuxcnc"],
        stock_commands=namespace["_dmc2_stock_execution"],
    )
    live_plotter._dmc2_axis_run_guard = guard
    return guard
