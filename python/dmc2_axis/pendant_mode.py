"""AXIS toolbar, panel visibility, and pendant-mode HAL synchronization."""

from __future__ import annotations

from collections.abc import Mapping
from pathlib import Path

from .constants import (
    PENDANT_ICON_FILE,
    PENDANT_MODE_PIN,
    PENDANT_MODE_OPERATION_ID,
    PENDANT_WIDGET_PATH,
)
from .operation_catalog import project_catalog_path, read_operations
from .recovery_contract import RecoveryOperationCode
from .recovery_ui import (
    RECOVERY_OPERATION_CONTRACTS,
    RecoveryUiNotice,
    present_recovery_ui_error,
    register_recovery_widget_command,
)
from .ui_fault import AxisUiFault, AxisUiFaultKind


class PendantModeBinding:
    """Keep the AXIS panel, toolbar indication, and HAL request synchronized."""

    def __init__(
        self,
        *,
        component,
        show_panel_variable,
        toggle_panel,
        tk,
        widget_path: str,
        inactive_image,
        active_image,
        namespace,
    ) -> None:
        self.component = component
        self.show_panel_variable = show_panel_variable
        self.toggle_panel = toggle_panel
        self.tk = tk
        self.widget_path = widget_path
        self.inactive_image = inactive_image
        self.active_image = active_image
        self.namespace = namespace
        self.menu_index: int | None = None
        self.requested = False
        self.transition_error_notice = RecoveryUiNotice(namespace)

    def _configure_controls(self, *, active: bool) -> None:
        options = [
            self.widget_path,
            "configure",
            "-relief",
            "sunken" if active else "link",
            "-state",
            "normal",
        ]
        image = self.active_image if active else self.inactive_image
        if image is None:
            options.extend(("-text", "Pendant"))
        else:
            options.extend(("-image", str(image)))
        self.tk.call(*options)
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
        self._set_panel_visible(False)

    def synchronize_visibility(self) -> None:
        """Keep the requested mode and panel visible without a readiness gate."""
        if self.requested:
            # Panel rendering is presentation only. It must never revoke the
            # already requested HAL control mode if Tk raises an exception.
            self._set_panel_visible(True)
        elif bool(self.show_panel_variable.get()):
            self._disarm_and_hide()

        self._configure_controls(active=self.requested)

    def set_enabled(self, enabled: bool) -> bool:
        enabled = bool(enabled)
        if enabled:
            self.component[PENDANT_MODE_PIN] = True
            self.requested = True
            self.synchronize_visibility()
            return True

        self._disarm_and_hide()
        self._configure_controls(active=False)
        return False

    def toggle(self, *event):
        try:
            self.set_enabled(not self.requested)
        except Exception as error:
            self.transition_error_notice.present(
                fault=AxisUiFault(
                    AxisUiFaultKind.PENDANT_MODE_UI_TRANSITION_FAILED,
                    error,
                )
            )
        else:
            self.transition_error_notice.clear()
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
    component[PENDANT_MODE_PIN] = False

    root_window = namespace["root_window"]
    tkinter_module = namespace["Tkinter"]
    pendant_contract = RECOVERY_OPERATION_CONTRACTS[
        RecoveryOperationCode.PENDANT_MODE
    ]
    binding = PendantModeBinding(
        component=component,
        show_panel_variable=namespace["vars"].show_pyvcppanel,
        toggle_panel=namespace["commands"].toggle_show_pyvcppanel,
        tk=root_window.tk,
        widget_path=PENDANT_WIDGET_PATH,
        inactive_image=None,
        active_image=None,
        namespace=namespace,
    )
    command_name = root_window.register(binding.toggle)
    button_options = [
        "Button",
        PENDANT_WIDGET_PATH,
        "-command",
        command_name,
        "-helptext",
        f"Toggle {pendant_contract.label}: arm/show or disarm/hide",
        "-relief",
        "link",
        "-state",
        "normal",
        "-takefocus",
        0,
    ]
    button_options.extend(("-text", pendant_contract.label))
    root_window.tk.call(*button_options)
    root_window.tk.call(
        "pack",
        PENDANT_WIDGET_PATH,
        "-side",
        "left",
        "-after",
        ".toolbar.clear_plot",
    )
    register_recovery_widget_command(
        namespace,
        RecoveryOperationCode.PENDANT_MODE,
        command_name,
    )
    live_plotter._dmc2_pendant_mode_binding = binding

    # The essential text control now exists before any auxiliary catalog,
    # icon, key-binding, or menu integration can fail.
    operations = read_operations(project_catalog_path(str(namespace["rcfile"])))
    operation = operations.get(PENDANT_MODE_OPERATION_ID)
    if operation is None:
        raise RuntimeError(f"Operation is missing: {PENDANT_MODE_OPERATION_ID}")
    if not pendant_contract.matches(operation):
        raise RuntimeError(
            f"Operation has invalid Pendant Mode contract: {operation!r}"
        )

    icon_path = Path(str(namespace["rcfile"])).resolve().with_name(PENDANT_ICON_FILE)
    icon_error = None
    try:
        if not icon_path.is_file():
            raise RuntimeError(f"Pendant toolbar icon is missing: {icon_path}")
        binding.inactive_image = tkinter_module.BitmapImage(
            master=root_window,
            file=str(icon_path),
            foreground="#202020",
        )
        binding.active_image = tkinter_module.BitmapImage(
            master=root_window,
            file=str(icon_path),
            foreground="#08752d",
        )
    except Exception as error:
        icon_error = error
    if icon_error is not None:
        present_recovery_ui_error(
            namespace,
            fault=AxisUiFault(
                AxisUiFaultKind.PENDANT_MODE_ICON_LOAD_FAILED,
                icon_error,
            )
        )
    try:
        root_window.bind("<Control-e>", binding.toggle)
    except Exception as error:
        present_recovery_ui_error(
            namespace,
            fault=AxisUiFault(
                AxisUiFaultKind.PENDANT_MODE_KEY_BINDING_FAILED,
                error,
            )
        )
    try:
        binding.menu_index = _replace_pyvcppanel_menu_command(namespace, command_name)
    except Exception as error:
        present_recovery_ui_error(
            namespace,
            fault=AxisUiFault(
                AxisUiFaultKind.PENDANT_MODE_MENU_BINDING_FAILED,
                error,
            )
        )
    try:
        binding.synchronize_visibility()
    except Exception as error:
        binding.transition_error_notice.present(
            fault=AxisUiFault(
                AxisUiFaultKind.PENDANT_MODE_INITIAL_PRESENTATION_FAILED,
                error,
            )
        )
    return binding
