"""AXIS toolbar, panel visibility, and pendant-mode HAL synchronization."""

from __future__ import annotations

from collections.abc import Mapping
from pathlib import Path

from .constants import (
    CONTROLLER_AVAILABLE_PIN,
    CONTROLLER_READY_PIN,
    PENDANT_ICON_FILE,
    PENDANT_MODE_PIN,
    PENDANT_WIDGET_PATH,
    READINESS_POLL_MILLISECONDS,
)


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
        del available
        self.tk.call(
            self.widget_path,
            "configure",
            "-image",
            str(self.active_image if active else self.inactive_image),
            "-relief",
            "sunken" if active else "link",
            "-state",
            "normal",
        )
        if self.menu_index is not None:
            self.tk.call(
                ".menu.view",
                "entryconfigure",
                self.menu_index,
                "-state",
                "normal",
            )

    def _set_panel_visible(self, visible: bool) -> None:
        visible = bool(visible)
        if bool(self.show_panel_variable.get()) == visible:
            return
        self.show_panel_variable.set(visible)
        self.toggle_panel()

    def _disarm_and_hide(self) -> None:
        self.component[PENDANT_MODE_PIN] = False
        self.requested = False
        self.enabled = False
        self._set_panel_visible(False)

    def synchronize_readiness(self) -> None:
        """Make UI visibility follow the controller's acknowledged state."""
        available = self._pin(CONTROLLER_AVAILABLE_PIN)
        ready = self._pin(CONTROLLER_READY_PIN)

        if self.requested:
            try:
                self._set_panel_visible(True)
            except Exception:
                self._disarm_and_hide()
                raise
            self.enabled = available and ready
        elif self.enabled or bool(self.show_panel_variable.get()):
            self._disarm_and_hide()

        self._configure_controls(active=self.requested, available=available)

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
