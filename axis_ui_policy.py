"""DMC2-specific AXIS notification behavior.

The realtime limit gate intentionally asserts ``motion.jog-stop-immediate``
before the userspace supervisor begins the already-validated bounce. LinuxCNC
reports that intentional stop through AXIS's error channel as an operator
error. Keep the realtime stop and its journal evidence, but do not present that
one exact, expected message as an operator error popup in AXIS.
"""

from __future__ import annotations

from collections.abc import Mapping
from pathlib import Path


EXPECTED_LIMIT_STOP_MESSAGE = "Jog aborted by jog-stop-immediate"
PENDANT_MODE_PIN = "pendant-mode-enabled"
CONTROLLER_AVAILABLE_PIN = "controller-available"
CONTROLLER_READY_PIN = "controller-ready"
PENDANT_WIDGET_PATH = ".toolbar.dmc2_pendant_mode"
PENDANT_ICON_FILE = "pendant_icon.xbm"
READINESS_POLL_MILLISECONDS = 20
REQUIRED_LINUXCNC_VERSION = "2.9.10"
ERROR_CHANNEL_KIND_DEFINITIONS = (
    ("NML_ERROR", 1, "error"),
    ("NML_TEXT", 2, "info"),
    ("NML_DISPLAY", 3, "info"),
    ("OPERATOR_ERROR", 11, "error"),
    ("OPERATOR_TEXT", 12, "info"),
    ("OPERATOR_DISPLAY", 13, "info"),
)


def error_channel_kind_catalog(linuxcnc_module) -> dict[int, tuple[str, str]]:
    """Return the complete public 2.9.10 error-channel type catalog."""
    version = str(getattr(linuxcnc_module, "version", ""))
    if version != REQUIRED_LINUXCNC_VERSION:
        raise RuntimeError(
            f"DMC2 requires LinuxCNC {REQUIRED_LINUXCNC_VERSION}; loaded {version or 'unknown'}"
        )
    catalog: dict[int, tuple[str, str]] = {}
    for name, expected_value, severity in ERROR_CHANNEL_KIND_DEFINITIONS:
        try:
            value = int(getattr(linuxcnc_module, name))
        except (AttributeError, TypeError, ValueError) as error:
            raise RuntimeError(
                f"LinuxCNC {REQUIRED_LINUXCNC_VERSION} omitted error-channel type {name}"
            ) from error
        if value != expected_value:
            raise RuntimeError(
                f"LinuxCNC {REQUIRED_LINUXCNC_VERSION} error-channel type {name} "
                f"must equal {expected_value}, found {value}"
            )
        if value in catalog:
            raise RuntimeError(
                f"LinuxCNC error-channel types {catalog[value][0]} and {name} both equal {value}"
            )
        catalog[value] = (name, severity)
    if len(catalog) != len(ERROR_CHANNEL_KIND_DEFINITIONS):
        raise RuntimeError("LinuxCNC error-channel catalog is incomplete")
    return catalog


class PendantModeBinding:
    """Keep the AXIS panel, toolbar indication, and HAL request synchronized."""

    def __init__(
        self,
        *,
        component,
        show_panel_variable,
        toggle_panel,
        root_window,
        tk,
        widget_path: str,
        inactive_image,
        active_image,
    ) -> None:
        self.component = component
        self.show_panel_variable = show_panel_variable
        self.toggle_panel = toggle_panel
        self.root_window = root_window
        self.tk = tk
        self.widget_path = widget_path
        self.inactive_image = inactive_image
        self.active_image = active_image
        self.menu_index: int | None = None
        self.requested = False
        self.enabled = False
        self.poll_after_id = None

    def _pin(self, name: str) -> bool:
        try:
            return bool(self.component[name])
        except (KeyError, RuntimeError):
            return False

    def _configure_controls(self, *, active: bool, available: bool) -> None:
        self.tk.call(
            self.widget_path,
            "configure",
            "-image",
            str(self.active_image if active else self.inactive_image),
            "-relief",
            "sunken" if active else "link",
            "-state",
            "normal" if available else "disabled",
        )
        if self.menu_index is not None:
            self.tk.call(
                ".menu.view",
                "entryconfigure",
                self.menu_index,
                "-state",
                "normal" if available else "disabled",
            )

    def _set_panel_visible(self, visible: bool) -> None:
        visible = bool(visible)
        if bool(self.show_panel_variable.get()) == visible:
            return
        self.show_panel_variable.set(visible)
        self.toggle_panel()

    def _disarm_and_hide(self) -> None:
        # Remove the HAL request before changing anything cosmetic.
        self.component[PENDANT_MODE_PIN] = False
        self.requested = False
        self.enabled = False
        self._set_panel_visible(False)

    def synchronize_readiness(self) -> None:
        """Make UI visibility follow the controller's acknowledged state."""
        available = self._pin(CONTROLLER_AVAILABLE_PIN)
        ready = self._pin(CONTROLLER_READY_PIN)

        if self.requested:
            if not available:
                self._disarm_and_hide()
            elif ready and not self.enabled:
                try:
                    self._set_panel_visible(True)
                except Exception:
                    self._disarm_and_hide()
                    raise
                self.enabled = True
        elif self.enabled or bool(self.show_panel_variable.get()):
            self._disarm_and_hide()

        self._configure_controls(
            active=self.requested,
            available=available,
        )

    def poll_readiness(self) -> None:
        self.synchronize_readiness()
        self.poll_after_id = self.root_window.after(
            READINESS_POLL_MILLISECONDS,
            self.poll_readiness,
        )

    def start_readiness_poll(self) -> None:
        self.synchronize_readiness()
        self.poll_after_id = self.root_window.after(
            READINESS_POLL_MILLISECONDS,
            self.poll_readiness,
        )

    def set_enabled(self, enabled: bool) -> bool:
        enabled = bool(enabled)
        if enabled:
            # The panel remains hidden until both the pre-arm availability
            # signal and the post-arm control-ready acknowledgement are true.
            self.synchronize_readiness()
            if not self._pin(CONTROLLER_AVAILABLE_PIN):
                self._disarm_and_hide()
                self._configure_controls(active=False, available=False)
                return False
            self.component[PENDANT_MODE_PIN] = True
            self.requested = True
            self.synchronize_readiness()
            return self.enabled

        self._disarm_and_hide()
        self._configure_controls(
            active=False,
            available=self._pin(CONTROLLER_AVAILABLE_PIN),
        )
        return False

    def toggle(self, *event):
        self.set_enabled(not (self.requested or self.enabled))
        if event:
            return "break"
        return None


def _replace_pyvcppanel_menu_command(namespace, command_name: str) -> int:
    root_window = namespace["root_window"]
    tkinter_module = namespace["Tkinter"]
    menu_path = ".menu.view"
    last_index = int(root_window.tk.call(menu_path, "index", "end"))
    for index in range(last_index + 1):
        try:
            variable = root_window.tk.call(
                menu_path,
                "entrycget",
                index,
                "-variable",
            )
        except tkinter_module.TclError:
            continue
        if str(variable).split("::")[-1] == "show_pyvcppanel":
            root_window.tk.call(
                menu_path,
                "entryconfigure",
                index,
                "-command",
                command_name,
            )
            return index
    raise RuntimeError("AXIS PyVCP visibility menu entry was not found")


def install_axis_pendant_mode(namespace: Mapping[str, object]) -> PendantModeBinding:
    """Add the fail-closed Pendant Mode pin and toolbar toggle to AXIS."""
    live_plotter = namespace["live_plotter"]
    existing = getattr(live_plotter, "_dmc2_pendant_mode_binding", None)
    if existing is not None:
        return existing

    component = namespace["comp"]
    hal_module = namespace["hal"]
    component.newpin(PENDANT_MODE_PIN, hal_module.HAL_BIT, hal_module.HAL_OUT)
    component.newpin(
        CONTROLLER_AVAILABLE_PIN,
        hal_module.HAL_BIT,
        hal_module.HAL_IN,
    )
    component.newpin(
        CONTROLLER_READY_PIN,
        hal_module.HAL_BIT,
        hal_module.HAL_IN,
    )
    component[PENDANT_MODE_PIN] = False

    root_window = namespace["root_window"]
    tkinter_module = namespace["Tkinter"]
    icon_path = Path(str(namespace["rcfile"])).resolve().with_name(PENDANT_ICON_FILE)
    if not icon_path.is_file():
        raise RuntimeError(f"Pendant toolbar icon is missing: {icon_path}")

    inactive_image = tkinter_module.BitmapImage(
        master=root_window,
        file=str(icon_path),
        foreground="#202020",
    )
    active_image = tkinter_module.BitmapImage(
        master=root_window,
        file=str(icon_path),
        foreground="#08752d",
    )
    binding = PendantModeBinding(
        component=component,
        show_panel_variable=namespace["vars"].show_pyvcppanel,
        toggle_panel=namespace["commands"].toggle_show_pyvcppanel,
        root_window=root_window,
        tk=root_window.tk,
        widget_path=PENDANT_WIDGET_PATH,
        inactive_image=inactive_image,
        active_image=active_image,
    )
    command_name = root_window.register(binding.toggle)
    root_window.tk.call(
        "Button",
        PENDANT_WIDGET_PATH,
        "-command",
        command_name,
        "-helptext",
        "Toggle Pendant Mode: arm/show or disarm/hide",
        "-image",
        str(inactive_image),
        "-relief",
        "link",
        "-state",
        "disabled",
        "-takefocus",
        0,
    )
    root_window.tk.call(
        "pack",
        PENDANT_WIDGET_PATH,
        "-side",
        "left",
        "-after",
        ".toolbar.clear_plot",
    )
    root_window.bind("<Control-e>", binding.toggle)
    binding.menu_index = _replace_pyvcppanel_menu_command(namespace, command_name)
    binding.start_readiness_poll()
    live_plotter._dmc2_pendant_mode_binding = binding
    return binding


def should_suppress_notification(kind, message: str, linuxcnc_module) -> bool:
    return (
        kind in (linuxcnc_module.NML_ERROR, linuxcnc_module.OPERATOR_ERROR)
        and message.strip() == EXPECTED_LIMIT_STOP_MESSAGE
    )


def install_axis_ui_policy(namespace: Mapping[str, object]) -> None:
    """Install once into the globals supplied by AXIS's USER_COMMAND_FILE."""
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
        # AXIS normally anchors notifications at bottom-right, directly over
        # the PyVCP status panel. Keep rare actionable notifications visible
        # on the opposite side of the application instead.
        notifications.place_configure(
            relx=0,
            rely=1,
            x=20,
            y=-20,
            anchor="sw",
        )

    def filtered_error_task():
        try:
            error = error_channel.poll()
            while error:
                try:
                    kind, raw_message = error
                    kind = int(kind)
                    message = str(raw_message)
                except (TypeError, ValueError) as malformed:
                    print(
                        "DMC2_LINUXCNC_ERROR_CHANNEL "
                        f"kind=malformed name=UNKNOWN severity=error suppressed=0 "
                        f"record={error!r} exception={malformed!r}",
                        flush=True,
                    )
                    notifications.add("error", f"Malformed LinuxCNC error record: {error!r}")
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
                error = error_channel.poll()
        except Exception as error:
            print(
                "DMC2_LINUXCNC_ERROR_CHANNEL "
                f"kind=poll_failure name=UNKNOWN severity=error suppressed=0 "
                f"exception={error!r}",
                flush=True,
            )
            notifications.add("error", f"LinuxCNC error-channel polling failed: {error}")
        finally:
            live_plotter.error_after = live_plotter.win.after(
                200,
                filtered_error_task,
            )

    notifications.add = add_without_covering_status_panel
    live_plotter.error_task = filtered_error_task
    live_plotter._dmc2_ui_policy_installed = True
