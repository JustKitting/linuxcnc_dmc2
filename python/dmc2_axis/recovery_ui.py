"""Validate that every typed recovery route resolves to visible operator UI."""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from enum import Enum
from pathlib import Path

from .diagnostic_journal import RecoveryRoute
from .operation_catalog import Operation, project_catalog_path, read_operations
from .recovery_contract import (
    RecoveryClassCode,
    RecoveryOperationCode,
    recovery_contract,
    validate_recovery_contract_model,
)
from .ui_fault import AxisUiFault, AxisUiFaultKind, validate_axis_ui_fault_model


EMERGENCY_RELAUNCH_RECOVERY_TEXT = (
    "Recovery class: RELAUNCH_APPLICATION\n"
    "Recovery transition: APPLICATION_RELAUNCHED\n"
    "Clear condition: correct the named installation/runtime cause and launch "
    "a matched DMC2 LinuxCNC session\n"
    "Recovery controls: DMC2 LinuxCNC -> Clear Fault -> Pendant Mode"
)


ALWAYS_AVAILABLE_OPERATIONS = (
    RecoveryOperationCode.ESTOP_RESET,
    RecoveryOperationCode.CLEAR_FAULT,
    RecoveryOperationCode.PENDANT_MODE,
)


class RecoveryUiPresentationChannel(Enum):
    """Closed outcomes for an operator-facing recovery presentation."""

    NOTIFICATION = "notification"
    DIALOG = "dialog"
    UNAVAILABLE = "unavailable"


@dataclass(frozen=True)
class RecoveryUiPresentation:
    """Result of attempting to put one typed recovery error on screen."""

    channel: RecoveryUiPresentationChannel
    widget: object | None = None

    @property
    def operator_visible(self) -> bool:
        return self.channel is not RecoveryUiPresentationChannel.UNAVAILABLE


@dataclass(frozen=True)
class RecoveryUiOperationContract:
    """Exact visible meaning and target of one admitted recovery operation."""

    label: str
    kind: str
    driver: str
    target: str
    ui_scope: str
    effects: tuple[str, ...]
    prerequisites: tuple[str, ...]
    ui_target: str

    def catalog_shape(self) -> tuple[object, ...]:
        return (
            self.label,
            self.kind,
            self.driver,
            self.target,
            self.ui_scope,
            self.effects,
            self.prerequisites,
            self.ui_target,
        )

    def matches(self, operation: Operation) -> bool:
        """Require a catalog row to preserve this exact UI operation."""
        return self.catalog_shape() == (
            operation.label,
            operation.kind,
            operation.driver,
            operation.target,
            operation.ui_scope,
            operation.effects,
            operation.prerequisites,
            operation.ui_target,
        )


# This is the consumer-side meaning of every operation admitted into a typed
# recovery route. It prevents a catalog row from retaining the right ID while
# silently pointing the operator at a different label, command, or widget.
RECOVERY_OPERATION_CONTRACTS = {
    RecoveryOperationCode.ABORT: RecoveryUiOperationContract(
        "Abort",
        "control",
        "linuxcnc.abort",
        "-",
        "base-toolbar",
        ("motion-stop", "spindle-stop"),
        ("running-session",),
        ".toolbar.program_stop",
    ),
    RecoveryOperationCode.ESTOP_RESET: RecoveryUiOperationContract(
        "Reset E-stop",
        "control",
        "linuxcnc.task-state",
        "estop-reset",
        "base-toolbar",
        ("state-change",),
        ("running-session", "physical-estop-released"),
        ".toolbar.machine_estop",
    ),
    RecoveryOperationCode.MACHINE_ON: RecoveryUiOperationContract(
        "Machine On",
        "control",
        "linuxcnc.task-state",
        "on",
        "base-toolbar",
        ("state-change",),
        ("running-session", "estop-clear"),
        ".toolbar.machine_power",
    ),
    RecoveryOperationCode.CLEAR_FAULT: RecoveryUiOperationContract(
        "Clear Fault",
        "control",
        "dmc2.clear-fault",
        "mesa-and-estop-reset",
        "base-toolbar",
        ("fault-clear", "state-change"),
        ("running-session", "physical-estop-released"),
        ".toolbar.dmc2_clear_fault",
    ),
    RecoveryOperationCode.PENDANT_MODE: RecoveryUiOperationContract(
        "Pendant Mode",
        "ui",
        "axis-ui",
        "pendant-mode-enable",
        "base-toolbar",
        ("operator-control",),
        ("running-session",),
        ".toolbar.dmc2_pendant_mode",
    ),
    RecoveryOperationCode.LAUNCH_APPLICATION: RecoveryUiOperationContract(
        "DMC2 LinuxCNC",
        "ui",
        "desktop-entry",
        "packaging/dmc2-linuxcnc.desktop",
        "system-applications",
        ("session-launch",),
        ("desktop-session",),
        "applications:DMC2 LinuxCNC",
    ),
    RecoveryOperationCode.HOME_ALL: RecoveryUiOperationContract(
        "Home All",
        "control",
        "linuxcnc.home-all",
        "-",
        "manual-tab-homing",
        ("axis-motion",),
        ("running-session", "estop-clear", "machine-on", "interpreter-idle"),
        ".pane.top.tabs.fmanual.dmc2_homing.home_all",
    ),
}


EXPECTED_WIDGET_COMMANDS = {
    RecoveryOperationCode.ABORT: "task_stop",
    RecoveryOperationCode.MACHINE_ON: "onoff_clicked",
}

REGISTERED_WIDGET_COMMANDS = frozenset(
    (RecoveryOperationCode.CLEAR_FAULT, RecoveryOperationCode.PENDANT_MODE)
)


def register_recovery_widget_command(
    namespace: Mapping[str, object], operation: RecoveryOperationCode, command: str
) -> None:
    """Record the exact Tcl callback installed for one DMC2 recovery control."""
    if operation not in REGISTERED_WIDGET_COMMANDS:
        raise RuntimeError(
            f"RECOVERY_UI_COMMAND_REGISTRATION_UNSUPPORTED: {operation!r}"
        )
    live_plotter = namespace["live_plotter"]
    registry = getattr(live_plotter, "_dmc2_recovery_widget_commands", None)
    if registry is None:
        registry = {}
        live_plotter._dmc2_recovery_widget_commands = registry
    previous = registry.get(operation)
    if previous is not None and previous != command:
        raise RuntimeError(
            "RECOVERY_UI_COMMAND_REGISTRATION_CONFLICT: "
            f"operation={operation.value} previous={previous!r} current={command!r}"
        )
    registry[operation] = command


def _validate_widget_command(
    namespace: Mapping[str, object],
    tk,
    operation_code: RecoveryOperationCode,
    target: str,
    *,
    stock_home_command: str | None = None,
) -> None:
    expected_command = EXPECTED_WIDGET_COMMANDS.get(operation_code)
    if expected_command is not None:
        command = str(tk.call(target, "cget", "-command"))
        if command != expected_command:
            raise RuntimeError(
                "RECOVERY_UI_WIDGET_COMMAND_INVALID: "
                f"operation={operation_code.value} widget={target} "
                f"expected={expected_command!r} actual={command!r}; "
                "action: restore the pinned AXIS command binding"
            )
        return
    if operation_code is RecoveryOperationCode.ESTOP_RESET:
        binding = str(tk.call("bind", target, "<Button-1>"))
        if "estop_clicked" not in binding.split():
            raise RuntimeError(
                "RECOVERY_UI_WIDGET_BINDING_INVALID: "
                f"operation={operation_code.value} widget={target} "
                f"binding={binding!r}; action: restore the pinned AXIS binding"
            )
        return
    if operation_code is RecoveryOperationCode.HOME_ALL:
        command = str(tk.call(target, "cget", "-command"))
        if stock_home_command is None or command != stock_home_command:
            raise RuntimeError(
                "RECOVERY_UI_HOME_COMMAND_INVALID: "
                f"widget={target} expected={stock_home_command!r} "
                f"actual={command!r}; action: restore the copied stock AXIS "
                "Home All command"
            )
        return
    if operation_code in REGISTERED_WIDGET_COMMANDS:
        registry = getattr(
            namespace["live_plotter"],
            "_dmc2_recovery_widget_commands",
            {},
        )
        expected_command = registry.get(operation_code)
        command = str(tk.call(target, "cget", "-command"))
        if expected_command is None or command != expected_command:
            raise RuntimeError(
                "RECOVERY_UI_REGISTERED_COMMAND_INVALID: "
                f"operation={operation_code.value} widget={target} "
                f"expected={expected_command!r} actual={command!r}; "
                "action: restore the exact DMC2 callback binding"
            )
        return
    raise RuntimeError(
        "RECOVERY_UI_WIDGET_OPERATION_UNHANDLED: "
        f"operation={operation_code.value} widget={target}; "
        "action: add its exact command contract before admitting it"
    )


def recovery_fallback_text(code: RecoveryClassCode) -> str:
    contract = recovery_contract(code)
    ui_path = " -> ".join(
        RECOVERY_OPERATION_CONTRACTS[operation].label
        for operation in contract.operations
    )
    return (
        f"Recovery class: {contract.identity}\n"
        f"Recovery transition: {contract.transition_identity}\n"
        f"Clear condition: {contract.clear_transition}\n"
        f"Recovery controls: {ui_path}"
    )


def recovery_route_text(route: RecoveryRoute) -> str:
    controls = " -> ".join(operation.label for operation in route.operations)
    return (
        f"Recovery class: {route.identity}\n"
        f"Recovery transition: {route.transition_identity}\n"
        f"Clear condition: {route.clear_transition}\n"
        f"Recovery controls: {controls}"
    )


def ensure_essential_recovery_controls(namespace: Mapping[str, object]) -> None:
    """Keep the three state-independent recovery controls operator-accessible."""
    tk = namespace["root_window"].tk
    for operation_code in ALWAYS_AVAILABLE_OPERATIONS:
        target = RECOVERY_OPERATION_CONTRACTS[operation_code].ui_target
        if not int(tk.call("winfo", "exists", target)):
            raise RuntimeError(
                "ESSENTIAL_RECOVERY_UI_WIDGET_MISSING: "
                f"operation={operation_code.value} widget={target}; "
                "action: restore the matched AXIS integration"
            )
        if not str(tk.call("winfo", "manager", target)):
            raise RuntimeError(
                "ESSENTIAL_RECOVERY_UI_WIDGET_NOT_LAID_OUT: "
                f"operation={operation_code.value} widget={target}; "
                "action: restore the control to the AXIS base toolbar"
            )
        tk.call(target, "configure", "-state", "normal")
        state = str(tk.call(target, "cget", "-state"))
        if state != "normal":
            raise RuntimeError(
                "ESSENTIAL_RECOVERY_UI_WIDGET_DISABLED: "
                f"operation={operation_code.value} widget={target} state={state!r}; "
                "action: restore this control to normal"
            )
        _validate_widget_command(namespace, tk, operation_code, target)


def _render_recovery_ui_error(
    fault: AxisUiFault,
    route: RecoveryRoute | None,
) -> tuple[AxisUiFault, str]:
    """Normalize one typed fault and render its complete recovery contract."""
    if not isinstance(fault, AxisUiFault):
        fault = AxisUiFault(
            AxisUiFaultKind.RECOVERY_UI_FAULT_TYPE_INVALID,
            f"received={fault!r}",
        )
    if route is not None and (
        not isinstance(route, RecoveryRoute) or route.code != fault.recovery_code
    ):
        fault = AxisUiFault(
            AxisUiFaultKind.RECOVERY_UI_FAULT_ROUTE_MISMATCH,
            (
                f"fault={fault.identity} expected={fault.recovery_code.name} "
                f"route={route!r}"
            ),
        )
        route = None
    try:
        recovery_text = (
            recovery_fallback_text(fault.recovery_code)
            if route is None
            else recovery_route_text(route)
        )
    except Exception as contract_error:
        original_fault = fault
        fault = AxisUiFault(
            AxisUiFaultKind.RECOVERY_UI_CONTRACT_RENDER_FAILED,
            f"{contract_error}; original_fault={original_fault!r}",
        )
        recovery_text = EMERGENCY_RELAUNCH_RECOVERY_TEXT
    return (
        fault,
        f"{fault.identity}\nCause: {fault.cause}\nAction: {fault.action}\n"
        f"{recovery_text}",
    )


def _log_recovery_ui_fault(fault: AxisUiFault) -> None:
    contract = fault.kind.contract.recovery
    operation_ids = " -> ".join(
        operation.value for operation in contract.operations
    )
    print(
        "DMC2_RECOVERY_UI_ERROR "
        f"identity={fault.identity!r} cause={fault.cause!r} "
        f"action={fault.action!r} recovery_class={fault.recovery_code.name!r} "
        f"recovery_transition={contract.transition_identity!r} "
        f"clear_condition={contract.clear_transition!r} "
        f"ui_path={operation_ids!r}",
        flush=True,
    )


def _show_recovery_dialog(
    namespace: Mapping[str, object],
    *,
    title: str,
    message: str,
    presentation_context: object,
) -> RecoveryUiPresentation:
    try:
        namespace["root_window"].tk.call(
            "nf_dialog",
            ".dmc2_recovery_error",
            title,
            message,
            "error",
            0,
            "OK",
        )
    except Exception as dialog_error:
        fault = AxisUiFault(
            AxisUiFaultKind.RECOVERY_UI_FALLBACK_DIALOG_FAILED,
            (
                f"dialog_error={dialog_error}; "
                f"presentation_context={presentation_context}"
            ),
        )
        _log_recovery_ui_fault(fault)
        print(
            "DMC2_RECOVERY_UI_UNAVAILABLE "
            f"retained_message={message!r} "
            f"fallback={EMERGENCY_RELAUNCH_RECOVERY_TEXT!r}",
            flush=True,
        )
        return RecoveryUiPresentation(RecoveryUiPresentationChannel.UNAVAILABLE)
    return RecoveryUiPresentation(RecoveryUiPresentationChannel.DIALOG)


def present_recovery_ui_fallback_dialog(
    namespace: Mapping[str, object],
    *,
    fault: AxisUiFault,
    route: RecoveryRoute | None = None,
) -> RecoveryUiPresentation:
    """Present directly through stock AXIS without re-entering notifications."""
    fault, message = _render_recovery_ui_error(fault, route)
    _log_recovery_ui_fault(fault)
    return _show_recovery_dialog(
        namespace,
        title="DMC2 recovery error",
        message=message,
        presentation_context=fault,
    )


def present_recovery_ui_error(
    namespace: Mapping[str, object],
    *,
    fault: AxisUiFault,
    route: RecoveryRoute | None = None,
    notification_add=None,
) -> RecoveryUiPresentation:
    fault, message = _render_recovery_ui_error(fault, route)
    _log_recovery_ui_fault(fault)
    try:
        raw_add = getattr(
            namespace["live_plotter"],
            "_dmc2_recovery_notification_add",
            None,
        )
        add = notification_add or raw_add or namespace["notifications"].add
        accepted = add("error", message)
        if accepted is False:
            raise RuntimeError(
                "RECOVERY_UI_NOTIFICATION_REJECTED: the AXIS notification "
                "boundary did not accept the typed recovery error"
            )
        return RecoveryUiPresentation(
            RecoveryUiPresentationChannel.NOTIFICATION,
            namespace["notifications"].widgets[-1],
        )
    except Exception as presentation_error:
        presentation_fault = AxisUiFault(
            AxisUiFaultKind.RECOVERY_UI_PRESENTATION_FAILED,
            presentation_error,
        )
        _log_recovery_ui_fault(presentation_fault)
        fallback_message = (
            f"{message}\n\n"
            f"{presentation_fault.identity}\n"
            f"Cause: {presentation_fault.cause}\n"
            f"Action: {presentation_fault.action}\n"
            f"{EMERGENCY_RELAUNCH_RECOVERY_TEXT}"
        )
        return _show_recovery_dialog(
            namespace,
            title="DMC2 recovery error",
            message=fallback_message,
            presentation_context=presentation_error,
        )


def clear_recovery_ui_error(
    namespace: Mapping[str, object], widget: object | None
) -> AxisUiFault | None:
    if widget is None:
        return None
    try:
        notifications = namespace["notifications"]
        if widget in notifications.widgets:
            notifications.remove(widget)
    except Exception as presentation_error:
        return AxisUiFault(
            AxisUiFaultKind.RECOVERY_UI_CLEAR_FAILED,
            presentation_error,
        )
    return None


class RecoveryUiNotice:
    """Own exactly one retryable UI error and remove it after recovery."""

    def __init__(self, namespace: Mapping[str, object], *, notification_add=None) -> None:
        self.namespace = namespace
        self.notification_add = notification_add
        self.presentation: RecoveryUiPresentation | None = None
        self.active_key: tuple[object, ...] | None = None
        self.clear_failure_key: tuple[object, ...] | None = None
        self.visibility_failure_key: tuple[object, ...] | None = None

    def is_visible(self) -> bool:
        if self.active_key is None:
            return False
        if self.presentation is None or not self.presentation.operator_visible:
            return False
        if self.presentation.channel is RecoveryUiPresentationChannel.DIALOG:
            # The stock fallback dialog was acknowledged for this active fault.
            # Retain that acknowledgment until its clear transition occurs.
            return True
        try:
            visible = self.presentation.widget in self.namespace["notifications"].widgets
        except Exception as error:
            fault = AxisUiFault(
                AxisUiFaultKind.RECOVERY_UI_PRESENTATION_FAILED,
                error,
            )
            failure_key = (fault.identity, str(fault.cause))
            if self.visibility_failure_key != failure_key:
                present_recovery_ui_fallback_dialog(
                    self.namespace,
                    fault=fault,
                )
                self.visibility_failure_key = failure_key
            # Visibility is unknown, so retain the existing notice reference.
            # Treating an inspection failure as absent would create duplicate
            # notices while the original may still be on screen.
            return True
        self.visibility_failure_key = None
        return visible

    def present(
        self,
        *,
        fault: AxisUiFault,
        route: RecoveryRoute | None = None,
    ) -> None:
        presentation_key = (repr(fault), repr(route))
        if self.active_key == presentation_key and self.is_visible():
            return
        if not self.clear():
            return
        self.active_key = presentation_key
        self.presentation = present_recovery_ui_error(
            self.namespace,
            fault=fault,
            route=route,
            notification_add=self.notification_add,
        )

    def present_fallback(
        self,
        *,
        fault: AxisUiFault,
        route: RecoveryRoute | None = None,
    ) -> None:
        presentation_key = ("fallback", repr(fault), repr(route))
        if self.active_key == presentation_key and self.is_visible():
            return
        if not self.clear():
            return
        self.active_key = presentation_key
        self.presentation = present_recovery_ui_fallback_dialog(
            self.namespace,
            fault=fault,
            route=route,
        )

    def clear(self) -> bool:
        widget = None if self.presentation is None else self.presentation.widget
        clear_fault = clear_recovery_ui_error(self.namespace, widget)
        if clear_fault is not None:
            failure_key = (clear_fault.identity, str(clear_fault.cause))
            if self.clear_failure_key != failure_key:
                present_recovery_ui_fallback_dialog(
                    self.namespace,
                    fault=clear_fault,
                )
                self.clear_failure_key = failure_key
            return False
        self.presentation = None
        self.active_key = None
        self.clear_failure_key = None
        self.visibility_failure_key = None
        return True


def recovery_contract_identity(routes: Sequence[RecoveryRoute]) -> tuple[object, ...]:
    return tuple(
        (
            route.code,
            route.identity,
            route.transition_code,
            route.transition_identity,
            route.clear_transition,
            route.operation_ids,
        )
        for route in routes
    )


def validate_local_recovery_ui(namespace: Mapping[str, object]) -> None:
    """Resolve every local fault class through the complete visible UI graph."""
    validate_recovery_contract_model()
    validate_axis_ui_fault_model()
    ensure_essential_recovery_controls(namespace)
    if set(RECOVERY_OPERATION_CONTRACTS) != set(RecoveryOperationCode):
        raise RuntimeError(
            "RECOVERY_UI_LOCAL_OPERATION_SET_INVALID: "
            f"expected={tuple(RecoveryOperationCode)!r} "
            f"actual={tuple(RECOVERY_OPERATION_CONTRACTS)!r}; "
            "action: restore every typed recovery operation's UI contract"
        )

    root_window = namespace["root_window"]
    tk = root_window.tk
    project_root = Path(str(namespace["rcfile"])).resolve().parents[2]
    operations = read_operations(project_catalog_path(str(namespace["rcfile"])))
    stock_home_command = str(namespace["widgets"].homebutton.cget("command"))
    if not stock_home_command:
        raise RuntimeError(
            "RECOVERY_UI_STOCK_HOME_COMMAND_MISSING: action: restore the pinned "
            "LinuxCNC AXIS Home All control"
        )

    fault_recovery_contracts = {
        kind: kind.contract.recovery for kind in AxisUiFaultKind
    }

    validated_operations = set()
    for class_code in RecoveryClassCode:
        contract = recovery_contract(class_code)
        for operation_code in contract.operations:
            if operation_code in validated_operations:
                continue
            operation = operations.get(operation_code.value)
            if operation is None:
                raise RuntimeError(
                    "RECOVERY_UI_OPERATION_MISSING: "
                    f"class={class_code.name} operation={operation_code.value}; "
                    "action: restore the matched operation catalog"
                )
            _validate_operation_ui(
                namespace,
                tk,
                project_root,
                operation,
                stock_home_command=stock_home_command,
            )
            validated_operations.add(operation_code)
    if validated_operations != set(RecoveryOperationCode):
        raise RuntimeError(
            "RECOVERY_UI_UNREACHABLE_OPERATION: "
            f"expected={tuple(RecoveryOperationCode)!r} "
            f"actual={tuple(sorted(validated_operations, key=lambda item: item.value))!r}; "
            "action: connect every operation to at least one recovery class"
        )
    for kind, contract in fault_recovery_contracts.items():
        missing_operations = tuple(
            operation
            for operation in contract.operations
            if operation not in validated_operations
        )
        if missing_operations:
            raise RuntimeError(
                "RECOVERY_UI_LOCAL_FAULT_PATH_UNAVAILABLE: "
                f"fault={kind.name} class={contract.identity} "
                f"operations={missing_operations!r}; action: restore every visible "
                "operation in this fault's explicit recovery path"
            )


def validate_recovery_ui(
    namespace: Mapping[str, object], routes: Sequence[RecoveryRoute]
) -> None:
    validate_local_recovery_ui(namespace)
    if not routes:
        raise RuntimeError(
            "RECOVERY_CLASS_CATALOG_UNAVAILABLE: no typed recovery routes were published; "
            "action: launch the matched DMC2 task monitor"
        )
    route_codes = tuple(route.code for route in routes)
    if route_codes != tuple(RecoveryClassCode):
        raise RuntimeError(
            "RECOVERY_UI_CLASS_SET_INVALID: "
            f"expected={tuple(RecoveryClassCode)!r} actual={route_codes!r}; "
            "action: restore the complete ordered recovery catalog"
        )
    observed_operation_ids = set()
    for route in routes:
        expected_route = recovery_contract(route.code)
        if (
            route.identity != expected_route.identity
            or route.slug != expected_route.slug
            or route.transition_code != expected_route.transition_code
            or route.transition_identity != expected_route.transition_identity
            or route.clear_transition != expected_route.clear_transition
            or route.operation_codes != expected_route.operations
            or route.operation_ids != expected_route.operation_ids
            or tuple(operation.id for operation in route.operations)
            != expected_route.operation_ids
        ):
            raise RuntimeError(
                "RECOVERY_UI_CLASS_CONTRACT_MISMATCH: "
                f"class={route.code.name}; action: restore the exact typed class, "
                "transition, and ordered UI path"
            )
        if route.operation_codes[-1] is not RecoveryOperationCode.PENDANT_MODE:
            raise RuntimeError(
                "RECOVERY_ROUTE_DOES_NOT_RETURN_TO_PENDANT: "
                f"class={route.identity} operations={route.operation_ids!r}; "
                "action: correct the typed recovery class"
            )
        for operation in route.operations:
            observed_operation_ids.add(operation.id)
    expected_operation_ids = {operation.value for operation in RecoveryOperationCode}
    if observed_operation_ids != expected_operation_ids:
        raise RuntimeError(
            "RECOVERY_UI_OPERATION_SET_INVALID: "
            f"expected={sorted(expected_operation_ids)!r} "
            f"actual={sorted(observed_operation_ids)!r}; "
            "action: restore the complete typed recovery catalog"
        )


def _validate_operation_ui(
    namespace: Mapping[str, object],
    tk,
    project_root: Path,
    operation: Operation,
    *,
    stock_home_command: str,
) -> None:
    try:
        operation_code = RecoveryOperationCode(operation.id)
    except ValueError as error:
        raise RuntimeError(
            "RECOVERY_UI_OPERATION_UNKNOWN: "
            f"operation={operation.id!r}; action: restore the closed operation catalog"
        ) from error
    expected = RECOVERY_OPERATION_CONTRACTS[operation_code]
    if not expected.matches(operation):
        raise RuntimeError(
            "RECOVERY_UI_OPERATION_CONTRACT_INVALID: "
            f"operation={operation.id} expected={expected!r} actual={operation!r}; "
            "action: restore the matched operation catalog"
        )
    if operation.ui_target.startswith("."):
        if not int(tk.call("winfo", "exists", operation.ui_target)):
            raise RuntimeError(
                "RECOVERY_UI_WIDGET_MISSING: "
                f"operation={operation.id} widget={operation.ui_target}; "
                "action: restore the matched AXIS integration"
            )
        manager = str(tk.call("winfo", "manager", operation.ui_target))
        if not manager:
            raise RuntimeError(
                "RECOVERY_UI_WIDGET_NOT_LAID_OUT: "
                f"operation={operation.id} widget={operation.ui_target}; "
                "action: restore the control to the AXIS base toolbar"
            )
        _validate_widget_command(
            namespace,
            tk,
            operation_code,
            operation.ui_target,
            stock_home_command=stock_home_command,
        )
        return
    if operation.driver == "desktop-entry":
        source = project_root / operation.target
        installed = Path.home() / ".local/share/applications" / source.name
        if (
            not source.is_file()
            or not installed.is_file()
            or source.read_bytes() != installed.read_bytes()
        ):
            raise RuntimeError(
                "RECOVERY_DESKTOP_ENTRY_MISSING_OR_STALE: "
                f"operation={operation.id} source={source} installed={installed}; "
                "action: restore the DMC2 LinuxCNC Applications entry"
            )
        return
    raise RuntimeError(
        "RECOVERY_UI_TARGET_UNSUPPORTED: "
        f"operation={operation.id} target={operation.ui_target!r}; "
        "action: assign a concrete AXIS widget or desktop entry"
    )
