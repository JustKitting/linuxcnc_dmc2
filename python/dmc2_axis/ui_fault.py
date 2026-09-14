"""Closed AXIS-local fault variants and their recovery classifications."""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum

from .recovery_contract import (
    RecoveryClassCode,
    RecoveryContract,
    RecoveryOperationCode,
    recovery_contract,
    validate_recovery_contract_model,
)


@dataclass(frozen=True)
class AxisUiFaultContract:
    """Stable operator meaning for one AXIS-local failure variant."""

    identity: str
    recovery_code: RecoveryClassCode
    action: str

    @property
    def recovery(self) -> RecoveryContract:
        """Resolve this fault directly to its clear transition and UI path."""
        return recovery_contract(self.recovery_code)


class AxisUiFaultKind(Enum):
    """Every AXIS-local exception that may cross an operator boundary."""

    OBJECT_MAPPER_INSTALL_FAILED = AxisUiFaultContract(
        "OBJECT_MAPPER_INSTALL_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "correct the named Object Mapper installation cause and relaunch DMC2; Abort, Clear Fault and Pendant Mode remain independent",
    )

    CUSTOM_SCRIPTS_INSTALL_FAILED = AxisUiFaultContract(
        "CUSTOM_SCRIPTS_INSTALL_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "correct the named Custom Scripts installation cause and relaunch DMC2; Abort, Clear Fault and Pendant Mode remain independent",
    )
    CUSTOM_SCRIPTS_REFRESH_FAILED = AxisUiFaultContract(
        "CUSTOM_SCRIPTS_REFRESH_FAILED",
        RecoveryClassCode.RECHECK_SOURCE,
        "correct the named display or parameter cause; the pane retries its display without issuing machine commands; use Abort, Clear Fault or Pendant Mode at any time",
    )
    CUSTOM_SCRIPTS_PREFERENCES_FAILED = AxisUiFaultContract(
        "CUSTOM_SCRIPTS_PREFERENCES_FAILED",
        RecoveryClassCode.RECHECK_SOURCE,
        "the parameter could not be saved for future sessions; correct the named file cause and edit the field again to retry; Pendant Mode remains available",
    )
    PROGRAM_RUN_REQUIRES_SCRIPT_PARAMETERS = AxisUiFaultContract(
        "PROGRAM_RUN_REQUIRES_SCRIPT_PARAMETERS",
        RecoveryClassCode.RECHECK_SOURCE,
        "open Custom Scripts, correct the named parameter and retry Run; Abort, Clear Fault and Pendant Mode remain available",
    )

    PROBE_MODE_UI_INSTALL_FAILED = AxisUiFaultContract(
        "PROBE_MODE_UI_INSTALL_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "correct the named Probe Mode UI cause and relaunch DMC2; Clear Fault and Pendant Mode remain independent",
    )

    BASE_RECOVERY_CONTROLS_INSTALL_FAILED = AxisUiFaultContract(
        "BASE_RECOVERY_CONTROLS_INSTALL_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "correct the named AXIS integration cause and relaunch DMC2 LinuxCNC; the stock E-stop controls remain available",
    )
    PENDANT_MODE_CONTROL_INSTALL_FAILED = AxisUiFaultContract(
        "PENDANT_MODE_CONTROL_INSTALL_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "correct the named AXIS/HAL cause and relaunch DMC2 LinuxCNC; the base recovery controls remain available",
    )
    SPINDLE_FEEDBACK_UI_INSTALL_FAILED = AxisUiFaultContract(
        "SPINDLE_FEEDBACK_UI_INSTALL_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "correct the named spindle-display cause and relaunch DMC2 LinuxCNC; recovery controls remain independent",
    )
    AXIS_NOTIFICATION_POLICY_INSTALL_FAILED = AxisUiFaultContract(
        "AXIS_NOTIFICATION_POLICY_INSTALL_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "correct the named error-presentation cause and relaunch DMC2 LinuxCNC",
    )
    AXIS_RUN_GUARD_INSTALL_FAILED = AxisUiFaultContract(
        "AXIS_RUN_GUARD_INSTALL_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "Run and Step are blocked; correct the named execution-control cause and relaunch from Applications; recovery controls install independently",
    )
    SCRIPT_LOADER_INSTALL_FAILED = AxisUiFaultContract(
        "SCRIPT_LOADER_INSTALL_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "correct the named script-loader cause and relaunch DMC2 LinuxCNC; Abort, Clear Fault, and Pendant Mode remain independent",
    )
    BASE_RECOVERY_CONTROL_REFRESH_FAILED = AxisUiFaultContract(
        "BASE_RECOVERY_CONTROL_REFRESH_FAILED",
        RecoveryClassCode.RECHECK_SOURCE,
        "use the visible Clear Fault or Pendant Mode control and correct the named indicator-refresh cause",
    )
    BASE_RECOVERY_CONTROL_RESCHEDULE_FAILED = AxisUiFaultContract(
        "BASE_RECOVERY_CONTROL_RESCHEDULE_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "use the visible recovery controls, then relaunch DMC2 LinuxCNC to restore live indicator checks",
    )
    CLEAR_FAULT_UI_COMMAND_FAILED = AxisUiFaultContract(
        "CLEAR_FAULT_UI_COMMAND_FAILED",
        RecoveryClassCode.CLEAR_CONTROLLER,
        "resolve the reported recovery condition and retry the visible Clear Fault control; Abort, E-stop, and Pendant Mode remain accessible",
    )
    PENDANT_MODE_UI_TRANSITION_FAILED = AxisUiFaultContract(
        "PENDANT_MODE_UI_TRANSITION_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "correct the named AXIS/HAL cause and retry the visible Pendant Mode control",
    )
    PENDANT_MODE_ICON_LOAD_FAILED = AxisUiFaultContract(
        "PENDANT_MODE_ICON_LOAD_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "restore the named icon file and relaunch DMC2 LinuxCNC; the text-labeled Pendant Mode control remains available",
    )
    PENDANT_MODE_KEY_BINDING_FAILED = AxisUiFaultContract(
        "PENDANT_MODE_KEY_BINDING_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "use the visible Pendant Mode toolbar control, correct the named key-binding cause, and relaunch DMC2 LinuxCNC",
    )
    PENDANT_MODE_MENU_BINDING_FAILED = AxisUiFaultContract(
        "PENDANT_MODE_MENU_BINDING_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "use the visible Pendant Mode toolbar control, correct the named menu-binding cause, and relaunch DMC2 LinuxCNC",
    )
    PENDANT_MODE_INITIAL_PRESENTATION_FAILED = AxisUiFaultContract(
        "PENDANT_MODE_INITIAL_PRESENTATION_FAILED",
        RecoveryClassCode.RECHECK_SOURCE,
        "retry the visible Pendant Mode toolbar control",
    )
    SPINDLE_FEEDBACK_REFRESH_FAILED = AxisUiFaultContract(
        "SPINDLE_FEEDBACK_REFRESH_FAILED",
        RecoveryClassCode.RECHECK_SOURCE,
        "correct the named spindle-feedback display cause; spindle controls and recovery controls remain independent",
    )
    SPINDLE_FEEDBACK_RESCHEDULE_FAILED = AxisUiFaultContract(
        "SPINDLE_FEEDBACK_RESCHEDULE_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "relaunch DMC2 LinuxCNC to restore live spindle-feedback polling",
    )
    PROGRAM_RUN_STATUS_UNAVAILABLE = AxisUiFaultContract(
        "PROGRAM_RUN_STATUS_UNAVAILABLE",
        RecoveryClassCode.RECHECK_SOURCE,
        "wait for a valid LinuxCNC status observation, then use the visible Pendant Mode control",
    )
    SCRIPT_CONTRACT_INSPECTION_FAILED = AxisUiFaultContract(
        "SCRIPT_CONTRACT_INSPECTION_FAILED",
        RecoveryClassCode.RECHECK_SOURCE,
        "correct or choose the machine-code file through the visible AXIS File Open control; Pendant Mode remains available",
    )
    SCRIPT_LOAD_SUBMISSION_FAILED = AxisUiFaultContract(
        "SCRIPT_LOAD_SUBMISSION_FAILED",
        RecoveryClassCode.ABORT_TASK,
        "use the visible Abort and Clear Fault controls, correct or choose the file through AXIS File Open, then return to Pendant Mode",
    )
    PROGRAM_RUN_REQUIRES_EXACT_LOADED_FILE = AxisUiFaultContract(
        "PROGRAM_RUN_REQUIRES_EXACT_LOADED_FILE",
        RecoveryClassCode.RECHECK_SOURCE,
        "load the intended file through the visible AXIS File Open control before retrying Run or Step; Pendant Mode remains available",
    )
    PROGRAM_RUN_REQUIRES_MACHINE_READY = AxisUiFaultContract(
        "PROGRAM_RUN_REQUIRES_MACHINE_READY",
        RecoveryClassCode.RESTORE_MACHINE,
        "use the visible E-stop Reset and Machine On controls required by this script, then return to Pendant Mode or retry Run",
    )
    PROGRAM_RUN_REQUIRES_IDLE_INTERPRETER = AxisUiFaultContract(
        "PROGRAM_RUN_REQUIRES_IDLE_INTERPRETER",
        RecoveryClassCode.ABORT_TASK,
        "use the visible Abort and Clear Fault controls or wait for the active program to finish, then return to Pendant Mode or retry Run",
    )
    PROGRAM_RUN_GUARD_EVALUATION_FAILED = AxisUiFaultContract(
        "PROGRAM_RUN_GUARD_EVALUATION_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "use the visible recovery controls, correct the named contract-evaluation cause, and relaunch DMC2 LinuxCNC before retrying Run",
    )
    PROGRAM_RUN_REQUIRES_HOMED_POSITION = AxisUiFaultContract(
        "PROGRAM_RUN_REQUIRES_HOMED_POSITION",
        RecoveryClassCode.ESTABLISH_POSITION,
        "use Reset E-stop, Machine On, and Home All; then return to Pendant Mode or retry Run or Step",
    )
    PROGRAM_RUN_SUBMISSION_FAILED = AxisUiFaultContract(
        "PROGRAM_RUN_SUBMISSION_FAILED",
        RecoveryClassCode.ABORT_TASK,
        "use Abort and Clear Fault, correct the named run cause, then return to Pendant Mode",
    )
    DIAGNOSTIC_JOURNAL_POLL_FAILED = AxisUiFaultContract(
        "DMC2_DIAGNOSTIC_JOURNAL_POLL_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "preserve the diagnostic journal, restore a readable regular file, then relaunch DMC2 LinuxCNC",
    )
    ERROR_JOURNAL_POLL_FAILED = AxisUiFaultContract(
        "DMC2_ERROR_JOURNAL_POLL_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "preserve the error journal, restore a readable regular file, then relaunch DMC2 LinuxCNC",
    )
    ERROR_JOURNAL_UNAVAILABLE = AxisUiFaultContract(
        "DMC2_ERROR_JOURNAL_UNAVAILABLE",
        RecoveryClassCode.RECHECK_SOURCE,
        "wait for the current task monitor to publish the exact error-journal transport header; Pendant Mode remains available",
    )
    ERROR_JOURNAL_RECORD_PRESENTATION_FAILED = AxisUiFaultContract(
        "DMC2_ERROR_JOURNAL_RECORD_PRESENTATION_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "preserve the raw record and relaunch DMC2 LinuxCNC with the matched reader",
    )
    DIAGNOSTIC_JOURNAL_RECORD_PRESENTATION_FAILED = AxisUiFaultContract(
        "DMC2_DIAGNOSTIC_JOURNAL_RECORD_PRESENTATION_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "preserve the raw record and relaunch DMC2 LinuxCNC with the matched reader",
    )
    RECOVERY_CLASS_CATALOG_UNAVAILABLE = AxisUiFaultContract(
        "DMC2_RECOVERY_CLASS_CATALOG_UNAVAILABLE",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "inspect the task-monitor error in the launch report, correct it, and relaunch DMC2 LinuxCNC",
    )
    RECOVERY_UI_CONTRACT_INVALID = AxisUiFaultContract(
        "DMC2_RECOVERY_UI_CONTRACT_INVALID",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "restore the named visible recovery operation and relaunch DMC2 LinuxCNC",
    )
    ESSENTIAL_RECOVERY_CONTROLS_UNAVAILABLE = AxisUiFaultContract(
        "DMC2_ESSENTIAL_RECOVERY_CONTROLS_UNAVAILABLE",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "use the stock E-stop control if available, correct the named AXIS control failure, and relaunch DMC2 LinuxCNC from Applications",
    )
    AXIS_DIAGNOSTIC_CALLBACK_FAILED = AxisUiFaultContract(
        "AXIS_DIAGNOSTIC_CALLBACK_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "use the visible recovery controls, correct the named AXIS presentation failure, and relaunch DMC2 LinuxCNC if it persists",
    )
    AXIS_DIAGNOSTIC_RESCHEDULE_FAILED = AxisUiFaultContract(
        "AXIS_DIAGNOSTIC_RESCHEDULE_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "use the visible recovery controls, then relaunch DMC2 LinuxCNC to restore diagnostic polling",
    )
    NOTIFICATION_DELIVERY_FAILED = AxisUiFaultContract(
        "AXIS_NOTIFICATION_DELIVERY_FAILED",
        RecoveryClassCode.RECHECK_SOURCE,
        "use the visible recovery controls and correct the named AXIS notification-delivery cause",
    )
    RECOVERY_UI_FAULT_TYPE_INVALID = AxisUiFaultContract(
        "RECOVERY_UI_FAULT_TYPE_INVALID",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "restore the closed AXIS fault type and relaunch DMC2 LinuxCNC",
    )
    RECOVERY_UI_FAULT_ROUTE_MISMATCH = AxisUiFaultContract(
        "RECOVERY_UI_FAULT_ROUTE_MISMATCH",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "restore the matched AXIS recovery contract and relaunch DMC2 LinuxCNC",
    )
    RECOVERY_UI_CONTRACT_RENDER_FAILED = AxisUiFaultContract(
        "RECOVERY_UI_CONTRACT_RENDER_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "restore the closed recovery model and relaunch DMC2 LinuxCNC",
    )
    RECOVERY_UI_PRESENTATION_FAILED = AxisUiFaultContract(
        "RECOVERY_UI_PRESENTATION_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "use the visible recovery controls, then relaunch DMC2 LinuxCNC to restore error presentation",
    )
    RECOVERY_UI_FALLBACK_DIALOG_FAILED = AxisUiFaultContract(
        "RECOVERY_UI_FALLBACK_DIALOG_FAILED",
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "use the visible recovery controls or the DMC2 LinuxCNC Applications launcher after correcting the named AXIS dialog cause",
    )
    RECOVERY_UI_CLEAR_FAILED = AxisUiFaultContract(
        "RECOVERY_UI_CLEAR_FAILED",
        RecoveryClassCode.RECHECK_SOURCE,
        "use the visible recovery controls; retry after the named stale-notification cause clears",
    )

    @property
    def contract(self) -> AxisUiFaultContract:
        return self.value


@dataclass(frozen=True)
class AxisUiFault:
    """One caught AXIS exception with a statically selected fault variant."""

    kind: AxisUiFaultKind
    cause: object

    def __post_init__(self) -> None:
        if not isinstance(self.kind, AxisUiFaultKind):
            raise TypeError(
                "AXIS_UI_FAULT_KIND_INVALID: "
                f"{self.kind!r}; action: select an AxisUiFaultKind variant"
            )

    @property
    def identity(self) -> str:
        return self.kind.contract.identity

    @property
    def recovery_code(self) -> RecoveryClassCode:
        return self.kind.contract.recovery_code

    @property
    def action(self) -> str:
        return self.kind.contract.action

    @property
    def clear_transition(self) -> str:
        return self.kind.contract.recovery.clear_transition

    @property
    def recovery_operations(self) -> tuple[RecoveryOperationCode, ...]:
        return self.kind.contract.recovery.operations


def validate_axis_ui_fault_model() -> None:
    """Reject aliases or incomplete local fault classifications."""
    validate_recovery_contract_model()
    if len(AxisUiFaultKind.__members__) != len(AxisUiFaultKind):
        raise RuntimeError(
            "AXIS_UI_FAULT_ALIAS_INVALID: action: restore unique local fault variants"
        )
    identities = tuple(kind.contract.identity for kind in AxisUiFaultKind)
    if len(identities) != len(set(identities)):
        raise RuntimeError(
            "AXIS_UI_FAULT_IDENTITY_DUPLICATE: action: restore unique local fault variants"
        )
    for kind in AxisUiFaultKind:
        contract = kind.contract
        recovery = contract.recovery
        if (
            not contract.identity
            or not isinstance(contract.recovery_code, RecoveryClassCode)
            or not contract.action
            or not recovery.clear_transition
            or not recovery.operations
            or recovery.operations[-1] is not RecoveryOperationCode.PENDANT_MODE
        ):
            raise RuntimeError(
                "AXIS_UI_FAULT_CONTRACT_INCOMPLETE: "
                f"kind={kind.name}; action: restore its identity, operator action, "
                "clear transition, and UI path ending in Pendant Mode"
            )
