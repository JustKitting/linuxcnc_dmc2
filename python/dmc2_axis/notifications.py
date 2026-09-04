"""Complete AXIS error-channel classification and presentation policy."""

from __future__ import annotations

from collections.abc import Mapping, MutableMapping

from .constants import (
    AGGREGATED_ERROR_PREFIXES,
    ERROR_CHANNEL_KIND_DEFINITIONS,
    EXPECTED_JOG_STOP_MESSAGES,
    REQUIRED_LINUXCNC_VERSION,
)
from .error_journal import ErrorJournalReader
from .diagnostic_journal import (
    DiagnosticJournalReader,
)
from .recovery_contract import RecoveryClassCode, RecoveryOperationCode
from .recovery_ui import (
    RecoveryUiNotice,
    ensure_essential_recovery_controls,
    present_recovery_ui_error,
    recovery_contract_identity,
    recovery_fallback_text,
    recovery_route_text,
    validate_local_recovery_ui,
    validate_recovery_ui,
)
from .ui_fault import AxisUiFault, AxisUiFaultKind

def error_channel_kind_catalog(linuxcnc_module) -> dict[int, tuple[str, str]]:
    """Return the complete public LinuxCNC 2.9.10 error-channel catalog."""
    version = str(getattr(linuxcnc_module, "version", ""))
    if version != REQUIRED_LINUXCNC_VERSION:
        raise RuntimeError(
            f"DMC2 requires LinuxCNC {REQUIRED_LINUXCNC_VERSION}; "
            f"loaded {version or 'unknown'}"
        )
    catalog: dict[int, tuple[str, str]] = {}
    for name, expected_value, severity in ERROR_CHANNEL_KIND_DEFINITIONS:
        try:
            value = int(getattr(linuxcnc_module, name))
        except (AttributeError, TypeError, ValueError) as error:
            raise RuntimeError(
                f"LinuxCNC {REQUIRED_LINUXCNC_VERSION} omitted "
                f"error-channel type {name}"
            ) from error
        if value != expected_value:
            raise RuntimeError(
                f"LinuxCNC {REQUIRED_LINUXCNC_VERSION} error-channel type {name} "
                f"must equal {expected_value}, found {value}"
            )
        if value in catalog:
            raise RuntimeError(
                f"LinuxCNC error-channel types {catalog[value][0]} and {name} "
                f"both equal {value}"
            )
        catalog[value] = (name, severity)
    if len(catalog) != len(ERROR_CHANNEL_KIND_DEFINITIONS):
        raise RuntimeError("LinuxCNC error-channel catalog is incomplete")
    return catalog


def should_suppress_notification(kind, message: str, linuxcnc_module) -> bool:
    if kind not in (linuxcnc_module.NML_ERROR, linuxcnc_module.OPERATOR_ERROR):
        return False
    normalized = message.strip()
    return normalized in EXPECTED_JOG_STOP_MESSAGES or normalized.startswith(
        AGGREGATED_ERROR_PREFIXES
    )


def install_axis_ui_policy(
    namespace: MutableMapping[str, object],
    *,
    journal_reader_factory=None,
    diagnostic_journal_reader_factory=None,
) -> None:
    """Install error-channel policy into AXIS's USER_COMMAND_FILE globals."""
    live_plotter = namespace["live_plotter"]
    if getattr(live_plotter, "_dmc2_ui_policy_installed", False):
        return

    if not isinstance(namespace, MutableMapping):
        raise RuntimeError("AXIS namespace must permit disabling its competing NML reader")
    linuxcnc_module = namespace["linuxcnc"]
    kind_catalog = error_channel_kind_catalog(linuxcnc_module)
    journal_reader = (journal_reader_factory or ErrorJournalReader)()
    diagnostic_reader = (
        diagnostic_journal_reader_factory or DiagnosticJournalReader
    )()
    namespace["e"] = None
    notifications = namespace["notifications"]
    original_add = notifications.add
    live_plotter._dmc2_recovery_notification_add = original_add
    active_diagnostic_widgets = {}
    checked_recovery_contract = None
    recovery_contract_error_identity = None
    recovery_contract_error_notice = RecoveryUiNotice(namespace)
    diagnostic_reader_error_notice = RecoveryUiNotice(namespace)
    error_reader_error_notice = RecoveryUiNotice(namespace)
    unexpected_poll_error_notice = RecoveryUiNotice(namespace)
    reschedule_error_notice = RecoveryUiNotice(namespace)
    diagnostic_clear_error_notice = RecoveryUiNotice(
        namespace,
        notification_add=original_add,
    )
    essential_controls_notice = RecoveryUiNotice(
        namespace,
        notification_add=original_add,
    )

    def add_with_delivery_status(icon_name, message):
        """Retain AXIS's stock non-control-covering placement and report failure."""
        try:
            original_add(icon_name, message)
        except Exception as error:
            present_recovery_ui_error(
                namespace,
                fault=AxisUiFault(
                    AxisUiFaultKind.NOTIFICATION_DELIVERY_FAILED,
                    error,
                ),
                notification_add=original_add,
            )
            return False
        return True

    def filtered_error_task():
        nonlocal checked_recovery_contract
        nonlocal recovery_contract_error_identity
        try:
            run_guard = getattr(live_plotter, "_dmc2_axis_run_guard", None)
            if run_guard is not None:
                run_guard.reconcile()
            try:
                ensure_essential_recovery_controls(namespace)
            except Exception as controls_error:
                essential_controls_notice.present(
                    fault=AxisUiFault(
                        AxisUiFaultKind.ESSENTIAL_RECOVERY_CONTROLS_UNAVAILABLE,
                        controls_error,
                    )
                )
            else:
                essential_controls_notice.clear()
            prefetched_diagnostic = None
            diagnostic_poll_failed = False
            try:
                # Load and validate the typed recovery catalog before presenting
                # any LinuxCNC message that refers to one of its class codes.
                prefetched_diagnostic = diagnostic_reader.poll()
            except Exception as polling_error:
                diagnostic_poll_failed = True
                kind = AxisUiFaultKind.DIAGNOSTIC_JOURNAL_POLL_FAILED
                print(
                    "DMC2_DIAGNOSTIC_PRESENTATION "
                    f"transition=reader-failure identity={kind.contract.identity} "
                    f"cause={polling_error!r} action={kind.contract.action!r} "
                    f"recovery_class={kind.contract.recovery_code.name!r}",
                    flush=True,
                )
                diagnostic_reader_error_notice.present(
                    fault=AxisUiFault(kind, polling_error),
                )
            error_poll_failed = False
            while True:
                try:
                    event = journal_reader.poll()
                except Exception as polling_error:
                    error_poll_failed = True
                    kind = AxisUiFaultKind.ERROR_JOURNAL_POLL_FAILED
                    print(
                        "DMC2_LINUXCNC_ERROR_CHANNEL "
                        f"kind=poll_failure name={kind.contract.identity} "
                        "severity=error suppressed=0 "
                        f"exception={polling_error!r}",
                        flush=True,
                    )
                    error_reader_error_notice.present(
                        fault=AxisUiFault(kind, polling_error),
                    )
                    break
                if event is None:
                    break
                try:
                    kind = int(event.message_type)
                    message = str(event.display_text())
                except Exception as malformed:
                    kind = AxisUiFaultKind.ERROR_JOURNAL_RECORD_PRESENTATION_FAILED
                    print(
                        "DMC2_LINUXCNC_ERROR_CHANNEL "
                        f"kind=malformed name={kind.contract.identity} "
                        "severity=error suppressed=0 "
                        f"record={event!r} exception={malformed!r}",
                        flush=True,
                    )
                    present_recovery_ui_error(
                        namespace,
                        fault=AxisUiFault(
                            kind,
                            f"{malformed}; raw record: {event!r}",
                        )
                    )
                else:
                    name, severity = kind_catalog.get(kind, ("UNKNOWN", "error"))
                    try:
                        recovery_route = diagnostic_reader.recovery_route(
                            event.recovery_code
                        )
                    except Exception as recovery_error:
                        recovery_text = recovery_fallback_text(event.recovery_code)
                        recovery_name = event.recovery_code.name
                        print(
                            "DMC2_LINUXCNC_ERROR_CHANNEL_RECOVERY "
                            f"sequence={event.sequence} status=unavailable "
                            f"cause={recovery_error!r} "
                            f"fallback={recovery_name}",
                            flush=True,
                        )
                    else:
                        recovery_text = recovery_route_text(recovery_route)
                        recovery_name = recovery_route.identity
                    suppressed = should_suppress_notification(
                        kind,
                        message,
                        linuxcnc_module,
                    )
                    print(
                        "DMC2_LINUXCNC_ERROR_CHANNEL "
                        f"sequence={event.sequence} kind={kind} name={name} "
                        f"severity={severity} serial={event.serial_number!r} "
                        f"operator_id={event.operator_id!r} "
                        f"recovery_class={recovery_name!r} "
                        f"suppressed={int(suppressed)} message={message!r}",
                        flush=True,
                    )
                    if not suppressed:
                        accepted = notifications.add(
                            severity,
                            f"{message}\n{recovery_text}",
                        )
                        if accepted is False:
                            raise RuntimeError(
                                "LINUXCNC_ERROR_NOTIFICATION_REJECTED: "
                                f"sequence={event.sequence}; action: use the visible "
                                "recovery controls and correct notification delivery"
                            )

            while not diagnostic_poll_failed:
                try:
                    if prefetched_diagnostic is not None:
                        diagnostic = prefetched_diagnostic
                        prefetched_diagnostic = None
                    else:
                        diagnostic = diagnostic_reader.poll()
                except Exception as polling_error:
                    kind = AxisUiFaultKind.DIAGNOSTIC_JOURNAL_POLL_FAILED
                    print(
                        "DMC2_DIAGNOSTIC_PRESENTATION "
                        f"transition=reader-failure identity={kind.contract.identity} "
                        f"cause={polling_error!r} action={kind.contract.action!r}",
                        flush=True,
                    )
                    diagnostic_reader_error_notice.present(
                        fault=AxisUiFault(kind, polling_error),
                    )
                    break
                if diagnostic is None:
                    break
                try:
                    transition = str(diagnostic.transition)
                    severity = str(diagnostic.severity)
                    message = str(diagnostic.notification_text())
                    identity = str(diagnostic.identity)
                    sequence = int(diagnostic.sequence)
                    raw_value = int(diagnostic.raw_value)
                    source = str(diagnostic.source)
                    domain = str(diagnostic.domain)
                    evidence = str(diagnostic.evidence)
                    recovery_class = str(diagnostic.recovery.identity)
                    recovery_path = str(diagnostic.recovery.ui_path)
                except Exception as malformed:
                    kind = (
                        AxisUiFaultKind.DIAGNOSTIC_JOURNAL_RECORD_PRESENTATION_FAILED
                    )
                    print(
                        "DMC2_DIAGNOSTIC_PRESENTATION "
                        f"transition=malformed identity={kind.contract.identity} "
                        f"record={diagnostic!r} cause={malformed!r} "
                        f"action={kind.contract.action!r}",
                        flush=True,
                    )
                    present_recovery_ui_error(
                        namespace,
                        fault=AxisUiFault(
                            kind,
                            f"{malformed}; raw record: {diagnostic!r}",
                        )
                    )
                    continue
                print(
                    "DMC2_DIAGNOSTIC_PRESENTATION "
                    f"sequence={sequence} transition={transition} "
                    f"severity={severity} identity={identity!r} source={source!r} "
                    f"domain={domain!r} raw={raw_value} "
                    f"recovery_class={recovery_class!r} "
                    f"ui_path={recovery_path!r} evidence={evidence!r}",
                    flush=True,
                )
            recovery_routes = diagnostic_reader.recovery_routes()
            if (
                not recovery_routes
                and not diagnostic_poll_failed
            ):
                checked_recovery_contract = None
                error_identity = ("catalog-unavailable",)
                try:
                    validate_local_recovery_ui(namespace)
                except Exception as local_recovery_error:
                    error_identity = (
                        "local-contract-invalid",
                        str(local_recovery_error),
                    )
                    if (
                        not recovery_contract_error_notice.is_visible()
                        or recovery_contract_error_identity != error_identity
                    ):
                        print(
                            "DMC2_RECOVERY_UI_CONTRACT "
                            f"classes={len(RecoveryClassCode)} status=invalid "
                            f"cause={local_recovery_error!r}",
                            flush=True,
                        )
                        recovery_contract_error_notice.present(
                            fault=AxisUiFault(
                                AxisUiFaultKind.RECOVERY_UI_CONTRACT_INVALID,
                                local_recovery_error,
                            )
                        )
                        recovery_contract_error_identity = error_identity
                else:
                    if (
                        not recovery_contract_error_notice.is_visible()
                        or recovery_contract_error_identity != error_identity
                    ):
                        print(
                            "DMC2_RECOVERY_UI_CONTRACT "
                            f"classes={len(RecoveryClassCode)} "
                            f"local_fault_types={len(AxisUiFaultKind)} "
                            f"ui_operations={len(RecoveryOperationCode)} "
                            "rust_catalog=unavailable status=invalid",
                            flush=True,
                        )
                        recovery_contract_error_notice.present(
                            fault=AxisUiFault(
                                AxisUiFaultKind.RECOVERY_CLASS_CATALOG_UNAVAILABLE,
                                "the task monitor has not published the closed recovery catalog",
                            )
                        )
                        recovery_contract_error_identity = error_identity
            elif recovery_routes:
                recovery_identity = recovery_contract_identity(recovery_routes)
                try:
                    # Re-evaluate every cycle. AXIS may alter toolbar state
                    # after initial construction, while these three recovery
                    # controls must remain operator-accessible in every state.
                    validate_recovery_ui(namespace, recovery_routes)
                except Exception as recovery_error:
                    checked_recovery_contract = None
                    error_identity = (recovery_identity, str(recovery_error))
                    if (
                        not recovery_contract_error_notice.is_visible()
                        or recovery_contract_error_identity != error_identity
                    ):
                        relaunch = next(
                            (
                                route
                                for route in recovery_routes
                                if route.identity == "RELAUNCH_APPLICATION"
                            ),
                            None,
                        )
                        print(
                            "DMC2_RECOVERY_UI_CONTRACT "
                            f"classes={len(recovery_routes)} status=invalid "
                            f"cause={recovery_error!r}",
                            flush=True,
                        )
                        recovery_contract_error_notice.present(
                            fault=AxisUiFault(
                                AxisUiFaultKind.RECOVERY_UI_CONTRACT_INVALID,
                                recovery_error,
                            ),
                            route=relaunch,
                        )
                        recovery_contract_error_identity = error_identity
                else:
                    if recovery_identity != checked_recovery_contract:
                        checked_recovery_contract = recovery_identity
                        recovery_contract_error_identity = None
                        recovery_contract_error_notice.clear()
                        print(
                            "DMC2_RECOVERY_UI_CONTRACT "
                            f"classes={len(recovery_routes)} "
                            f"local_fault_types={len(AxisUiFaultKind)} "
                            f"ui_operations={len(RecoveryOperationCode)} "
                            "rust_catalog=matched status=available",
                            flush=True,
                        )

            active_diagnostics = {
                tuple(diagnostic.active_key()): diagnostic
                for diagnostic in diagnostic_reader.active_events()
            }
            clear_failures = []
            for active_key, widget in tuple(active_diagnostic_widgets.items()):
                if active_key in active_diagnostics:
                    continue
                try:
                    if widget in notifications.widgets:
                        notifications.remove(widget)
                except Exception as clear_error:
                    clear_failures.append((active_key, clear_error))
                else:
                    active_diagnostic_widgets.pop(active_key, None)
            if clear_failures:
                diagnostic_clear_error_notice.present(
                    fault=AxisUiFault(
                        AxisUiFaultKind.RECOVERY_UI_CLEAR_FAILED,
                        "; ".join(
                            f"diagnostic={active_key!r} error={clear_error}"
                            for active_key, clear_error in clear_failures
                        ),
                    )
                )
            else:
                diagnostic_clear_error_notice.clear()

            for active_key, diagnostic in active_diagnostics.items():
                previous_widget = active_diagnostic_widgets.get(active_key)
                if (
                    previous_widget is not None
                    and previous_widget in notifications.widgets
                ):
                    continue
                accepted = notifications.add(
                    str(diagnostic.severity),
                    str(diagnostic.notification_text()),
                )
                if accepted is False:
                    raise RuntimeError(
                        "DMC2_DIAGNOSTIC_NOTIFICATION_REJECTED: "
                        f"diagnostic={active_key!r}; action: use the visible "
                        "recovery controls and correct notification delivery"
                    )
                active_diagnostic_widgets[active_key] = notifications.widgets[-1]
            if not error_poll_failed:
                if journal_reader.contract_ready():
                    error_reader_error_notice.clear()
                else:
                    journal_path = getattr(journal_reader, "path", "unknown")
                    error_reader_error_notice.present(
                        fault=AxisUiFault(
                            AxisUiFaultKind.ERROR_JOURNAL_UNAVAILABLE,
                            (
                                f"journal={journal_path}; the current task monitor "
                                "has not published the exact error-journal transport "
                                "header"
                            ),
                        )
                    )
            if diagnostic_reader.contract_ready():
                diagnostic_reader_error_notice.clear()
        except Exception as unexpected_error:
            unexpected_poll_error_notice.present(
                fault=AxisUiFault(
                    AxisUiFaultKind.AXIS_DIAGNOSTIC_CALLBACK_FAILED,
                    unexpected_error,
                )
            )
        else:
            unexpected_poll_error_notice.clear()
        try:
            live_plotter.error_after = live_plotter.win.after(
                200,
                filtered_error_task,
            )
        except Exception as error:
            reschedule_error_notice.present(
                fault=AxisUiFault(
                    AxisUiFaultKind.AXIS_DIAGNOSTIC_RESCHEDULE_FAILED,
                    error,
                )
            )

    notifications.add = add_with_delivery_status
    live_plotter.error_task = filtered_error_task
    live_plotter._dmc2_diagnostic_reader = diagnostic_reader
    live_plotter._dmc2_active_diagnostic_widgets = active_diagnostic_widgets
    live_plotter._dmc2_recovery_contract = lambda: checked_recovery_contract
    live_plotter._dmc2_ui_policy_installed = True
