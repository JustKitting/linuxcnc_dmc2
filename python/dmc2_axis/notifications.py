"""Complete AXIS error-channel classification and presentation policy."""

from __future__ import annotations

from collections.abc import Mapping

from .constants import (
    ERROR_CHANNEL_KIND_DEFINITIONS,
    EXPECTED_LIMIT_STOP_MESSAGE,
    REQUIRED_LINUXCNC_VERSION,
)


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
    return (
        kind in (linuxcnc_module.NML_ERROR, linuxcnc_module.OPERATOR_ERROR)
        and message.strip() == EXPECTED_LIMIT_STOP_MESSAGE
    )


def install_axis_ui_policy(namespace: Mapping[str, object]) -> None:
    """Install error-channel policy into AXIS's USER_COMMAND_FILE globals."""
    live_plotter = namespace["live_plotter"]
    if getattr(live_plotter, "_dmc2_ui_policy_installed", False):
        return

    error_channel = namespace["e"]
    linuxcnc_module = namespace["linuxcnc"]
    kind_catalog = error_channel_kind_catalog(linuxcnc_module)
    notifications = namespace["notifications"]
    original_add = notifications.add

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
            while True:
                try:
                    error = error_channel.poll()
                except Exception as polling_error:
                    print(
                        "DMC2_LINUXCNC_ERROR_CHANNEL "
                        "kind=poll_failure name=UNKNOWN severity=error suppressed=0 "
                        f"exception={polling_error!r}",
                        flush=True,
                    )
                    notifications.add(
                        "error",
                        f"LinuxCNC error-channel polling failed: {polling_error}",
                    )
                    break
                if error is None:
                    break
                try:
                    kind, raw_message = error
                    kind = int(kind)
                    message = str(raw_message)
                except Exception as malformed:
                    print(
                        "DMC2_LINUXCNC_ERROR_CHANNEL "
                        "kind=malformed name=UNKNOWN severity=error suppressed=0 "
                        f"record={error!r} exception={malformed!r}",
                        flush=True,
                    )
                    notifications.add(
                        "error", f"Malformed LinuxCNC error record: {error!r}"
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
                        f"kind={kind} name={name} severity={severity} "
                        f"suppressed={int(suppressed)} message={message!r}",
                        flush=True,
                    )
                    if not suppressed:
                        notifications.add(severity, message)
        finally:
            live_plotter.error_after = live_plotter.win.after(
                200,
                filtered_error_task,
            )

    notifications.add = add_without_covering_status_panel
    live_plotter.error_task = filtered_error_task
    live_plotter._dmc2_ui_policy_installed = True
