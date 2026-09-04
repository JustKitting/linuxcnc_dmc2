"""Closed AXIS-side mirror of the Rust recovery wire contract."""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum, IntEnum


class RecoveryClassCode(IntEnum):
    RECHECK_SOURCE = 1
    CLEAR_CONTROLLER = 2
    RESTORE_PENDANT = 3
    RELEASE_LIMIT = 4
    ABORT_TASK = 5
    RESTORE_MACHINE = 6
    RESET_SPINDLE = 7
    RELAUNCH_APPLICATION = 8
    ESTABLISH_POSITION = 9


class RecoveryTransitionCode(IntEnum):
    SOURCE_HEALTHY = 1
    CONTROLLER_FAULT_CLEARED = 2
    PENDANT_STREAM_RESTORED = 3
    LIMIT_RELEASED = 4
    TASK_IDLE = 5
    MACHINE_READY = 6
    SPINDLE_RESET = 7
    APPLICATION_RELAUNCHED = 8
    POSITION_KNOWN = 9


class RecoveryOperationCode(Enum):
    """Closed identities for every operator-visible recovery operation."""

    ABORT = "machine.abort"
    CLEAR_FAULT = "controller.clear-fault"
    ESTOP_RESET = "machine.estop-reset"
    MACHINE_ON = "machine.on"
    LAUNCH_APPLICATION = "application.launch"
    PENDANT_MODE = "controller.pendant-mode"
    HOME_ALL = "machine.home-all"


@dataclass(frozen=True)
class RecoveryContract:
    """One complete recovery class, clear transition, and ordered UI path."""

    code: RecoveryClassCode
    slug: str
    transition_code: RecoveryTransitionCode
    clear_transition: str
    operations: tuple[RecoveryOperationCode, ...]

    @property
    def identity(self) -> str:
        return self.code.name

    @property
    def transition_identity(self) -> str:
        return self.transition_code.name

    @property
    def operation_ids(self) -> tuple[str, ...]:
        return tuple(operation.value for operation in self.operations)


RECOVERY_CONTRACTS = (
    RecoveryContract(
        RecoveryClassCode.RECHECK_SOURCE,
        "recheck-source",
        RecoveryTransitionCode.SOURCE_HEALTHY,
        "a subsequent valid monitor evaluation no longer contains the exact typed issue",
        (RecoveryOperationCode.PENDANT_MODE,),
    ),
    RecoveryContract(
        RecoveryClassCode.CLEAR_CONTROLLER,
        "clear-controller",
        RecoveryTransitionCode.CONTROLLER_FAULT_CLEARED,
        "after the named cause is healthy, LinuxCNC's canonical E-stop Reset reaches the controller and its retained fault code returns to NONE",
        (RecoveryOperationCode.CLEAR_FAULT, RecoveryOperationCode.PENDANT_MODE),
    ),
    RecoveryContract(
        RecoveryClassCode.RESTORE_PENDANT,
        "restore-pendant",
        RecoveryTransitionCode.PENDANT_STREAM_RESTORED,
        "the Nano supplies a fresh coherent stream, Clear Fault is accepted, and both the bridge and controller pendant faults are absent",
        (RecoveryOperationCode.CLEAR_FAULT, RecoveryOperationCode.PENDANT_MODE),
    ),
    RecoveryContract(
        RecoveryClassCode.RELEASE_LIMIT,
        "release-limit",
        RecoveryTransitionCode.LIMIT_RELEASED,
        "after the physical cause is clear, Clear Fault is accepted and the configured startup backoff leaves both raw and safety-limit indications clear",
        (RecoveryOperationCode.CLEAR_FAULT, RecoveryOperationCode.PENDANT_MODE),
    ),
    RecoveryContract(
        RecoveryClassCode.ABORT_TASK,
        "abort-task",
        RecoveryTransitionCode.TASK_IDLE,
        "LinuxCNC accepts Abort, reports the interpreter idle, and the exact typed task issue is absent from the next valid evaluation",
        (
            RecoveryOperationCode.ABORT,
            RecoveryOperationCode.CLEAR_FAULT,
            RecoveryOperationCode.PENDANT_MODE,
        ),
    ),
    RecoveryContract(
        RecoveryClassCode.RESTORE_MACHINE,
        "restore-machine",
        RecoveryTransitionCode.MACHINE_READY,
        "after the physical E-stop is released, LinuxCNC reports E-stop reset and Machine On",
        (
            RecoveryOperationCode.ESTOP_RESET,
            RecoveryOperationCode.MACHINE_ON,
            RecoveryOperationCode.PENDANT_MODE,
        ),
    ),
    RecoveryContract(
        RecoveryClassCode.RESET_SPINDLE,
        "reset-spindle",
        RecoveryTransitionCode.SPINDLE_RESET,
        "after Abort and correction of the named drive cause, Clear Fault reaches the H100 reset input and its fault, block, and current-fault indications clear",
        (
            RecoveryOperationCode.ABORT,
            RecoveryOperationCode.CLEAR_FAULT,
            RecoveryOperationCode.PENDANT_MODE,
        ),
    ),
    RecoveryContract(
        RecoveryClassCode.RELAUNCH_APPLICATION,
        "relaunch-application",
        RecoveryTransitionCode.APPLICATION_RELAUNCHED,
        "after correction of the named installation or runtime cause, the Applications launcher starts a matching session that publishes a valid diagnostic catalog",
        (
            RecoveryOperationCode.LAUNCH_APPLICATION,
            RecoveryOperationCode.CLEAR_FAULT,
            RecoveryOperationCode.PENDANT_MODE,
        ),
    ),
    RecoveryContract(
        RecoveryClassCode.ESTABLISH_POSITION,
        "establish-position",
        RecoveryTransitionCode.POSITION_KNOWN,
        "after E-stop Reset and Machine On, Home All completes and LinuxCNC reports every configured joint homed",
        (
            RecoveryOperationCode.ESTOP_RESET,
            RecoveryOperationCode.MACHINE_ON,
            RecoveryOperationCode.HOME_ALL,
            RecoveryOperationCode.PENDANT_MODE,
        ),
    ),
)

RECOVERY_CONTRACTS_BY_CODE = {
    contract.code: contract for contract in RECOVERY_CONTRACTS
}


def recovery_contract(code: RecoveryClassCode) -> RecoveryContract:
    """Resolve a wire class without accepting an untyped integer or string."""
    if not isinstance(code, RecoveryClassCode):
        raise TypeError(
            "AXIS_RECOVERY_CLASS_TYPE_INVALID: "
            f"{code!r}; action: supply a RecoveryClassCode variant"
        )
    try:
        return RECOVERY_CONTRACTS_BY_CODE[code]
    except KeyError as error:
        raise RuntimeError(
            "AXIS_RECOVERY_CLASS_UNMAPPED: "
            f"{code!r}; action: restore the closed AXIS recovery contract"
        ) from error


def validate_recovery_contract_model() -> None:
    """Reject any incomplete, duplicate, or reordered local class model."""
    if (
        len(RecoveryClassCode.__members__) != len(RecoveryClassCode)
        or len(RecoveryTransitionCode.__members__) != len(RecoveryTransitionCode)
        or len(RecoveryOperationCode.__members__) != len(RecoveryOperationCode)
    ):
        raise RuntimeError(
            "AXIS_RECOVERY_ENUM_ALIAS_INVALID: action: restore unique class, "
            "transition, and operation variants"
        )
    expected_classes = tuple(RecoveryClassCode)
    expected_transitions = tuple(RecoveryTransitionCode)
    actual_classes = tuple(contract.code for contract in RECOVERY_CONTRACTS)
    actual_transitions = tuple(
        contract.transition_code for contract in RECOVERY_CONTRACTS
    )
    if actual_classes != expected_classes:
        raise RuntimeError(
            "AXIS_RECOVERY_CLASS_SET_INVALID: "
            f"expected={expected_classes!r} actual={actual_classes!r}; "
            "action: restore the closed AXIS recovery contract"
        )
    if actual_transitions != expected_transitions:
        raise RuntimeError(
            "AXIS_RECOVERY_TRANSITION_SET_INVALID: "
            f"expected={expected_transitions!r} actual={actual_transitions!r}; "
            "action: restore the closed AXIS recovery contract"
        )
    for contract in RECOVERY_CONTRACTS:
        if (
            not contract.slug
            or not contract.clear_transition
            or not contract.operations
            or len(set(contract.operations)) != len(contract.operations)
            or contract.operations[-1] is not RecoveryOperationCode.PENDANT_MODE
        ):
            raise RuntimeError(
                "AXIS_RECOVERY_CONTRACT_INCOMPLETE: "
                f"class={contract.identity}; action: restore its explicit clear "
                "transition and UI path"
            )
    used_operations = {
        operation
        for contract in RECOVERY_CONTRACTS
        for operation in contract.operations
    }
    if used_operations != set(RecoveryOperationCode):
        raise RuntimeError(
            "AXIS_RECOVERY_OPERATION_SET_INVALID: "
            f"expected={tuple(RecoveryOperationCode)!r} "
            f"actual={tuple(sorted(used_operations, key=lambda item: item.value))!r}; "
            "action: restore the closed recovery operation model"
        )
