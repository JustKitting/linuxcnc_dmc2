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
from .diagnostic_journal import DiagnosticJournalReader


def _reader_failure_text(identity: str, cause: object, action: str) -> str:
    return f"{identity}\nCause: {cause}\nAction: {action}"


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
    active_diagnostic_widgets = {}

    def add_without_covering_status_panel(icon_name, message):
        original_add(icon_name, message)
        notifications.place_configure(
            relx=0,
            rely=1,
            x=20,
            y=-20,
            anchor="sw",
        )

    def filtered_error_task():
        try:
            pending_diagnostic_asserts = {}
            while True:
                try:
                    event = journal_reader.poll()
                except Exception as polling_error:
                    identity = "DMC2_ERROR_JOURNAL_POLL_FAILED"
                    action = (
                        "preserve the error journal, restore a readable regular file, "
                        "then restart the task monitor and AXIS"
                    )
                    print(
                        "DMC2_LINUXCNC_ERROR_CHANNEL "
                        f"kind=poll_failure name={identity} severity=error suppressed=0 "
                        f"exception={polling_error!r}",
                        flush=True,
                    )
                    notifications.add(
                        "error",
                        _reader_failure_text(identity, polling_error, action),
                    )
                    break
                if event is None:
                    break
                try:
                    kind = int(event.message_type)
                    message = str(event.display_text())
                except Exception as malformed:
                    identity = "DMC2_ERROR_JOURNAL_RECORD_PRESENTATION_FAILED"
                    action = (
                        "preserve the raw record and restart AXIS with the matched "
                        "DMC2 reader"
                    )
                    print(
                        "DMC2_LINUXCNC_ERROR_CHANNEL "
                        f"kind=malformed name={identity} severity=error suppressed=0 "
                        f"record={event!r} exception={malformed!r}",
                        flush=True,
                    )
                    notifications.add(
                        "error",
                        _reader_failure_text(identity, malformed, action)
                        + f"\nRaw record: {event!r}",
                    )
                else:
                    name, severity = kind_catalog.get(kind, ("UNKNOWN", "error"))
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
                        f"suppressed={int(suppressed)} message={message!r}",
                        flush=True,
                    )
                    if not suppressed:
                        notifications.add(severity, message)

            while True:
                try:
                    diagnostic = diagnostic_reader.poll()
                except Exception as polling_error:
                    identity = "DMC2_DIAGNOSTIC_JOURNAL_POLL_FAILED"
                    action = (
                        "preserve the diagnostic journal, restore a readable regular "
                        "file, then restart the task monitor and AXIS"
                    )
                    print(
                        "DMC2_DIAGNOSTIC_PRESENTATION "
                        f"transition=reader-failure identity={identity} "
                        f"cause={polling_error!r} action={action!r}",
                        flush=True,
                    )
                    notifications.add(
                        "error",
                        _reader_failure_text(identity, polling_error, action),
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
                    active_key = tuple(diagnostic.active_key())
                except Exception as malformed:
                    failure_identity = (
                        "DMC2_DIAGNOSTIC_JOURNAL_RECORD_PRESENTATION_FAILED"
                    )
                    action = (
                        "preserve the raw record and restart AXIS with the matched "
                        "DMC2 reader"
                    )
                    print(
                        "DMC2_DIAGNOSTIC_PRESENTATION "
                        f"transition=malformed identity={failure_identity} "
                        f"record={diagnostic!r} cause={malformed!r} action={action!r}",
                        flush=True,
                    )
                    notifications.add(
                        "error",
                        _reader_failure_text(failure_identity, malformed, action)
                        + f"\nRaw record: {diagnostic!r}",
                    )
                    continue
                print(
                    "DMC2_DIAGNOSTIC_PRESENTATION "
                    f"sequence={sequence} transition={transition} "
                    f"severity={severity} identity={identity!r} source={source!r} "
                    f"domain={domain!r} raw={raw_value} evidence={evidence!r}",
                    flush=True,
                )
                if transition == "ASSERT":
                    pending_diagnostic_asserts[active_key] = (
                        sequence,
                        severity,
                        message,
                        active_key,
                    )
                else:
                    pending_diagnostic_asserts.pop(active_key, None)
                    widget = active_diagnostic_widgets.pop(active_key, None)
                    if widget is not None and widget in notifications.widgets:
                        notifications.remove(widget)

            for _sequence, severity, message, active_key in sorted(
                pending_diagnostic_asserts.values(),
                key=lambda pending: pending[0],
            ):
                previous_widget = active_diagnostic_widgets.get(active_key)
                if (
                    previous_widget is not None
                    and previous_widget in notifications.widgets
                ):
                    continue
                notifications.add(severity, message)
                active_diagnostic_widgets[active_key] = notifications.widgets[-1]
        finally:
            live_plotter.error_after = live_plotter.win.after(
                200,
                filtered_error_task,
            )

    notifications.add = add_without_covering_status_panel
    live_plotter.error_task = filtered_error_task
    live_plotter._dmc2_diagnostic_reader = diagnostic_reader
    live_plotter._dmc2_active_diagnostic_widgets = active_diagnostic_widgets
    live_plotter._dmc2_ui_policy_installed = True
