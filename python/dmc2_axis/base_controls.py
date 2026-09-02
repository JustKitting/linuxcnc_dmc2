"""The compact homing section in AXIS's stock Manual Control tab."""

from __future__ import annotations

from collections.abc import Mapping

from .constants import (
    CLEAR_FAULT_OPERATION_ID,
    CLEAR_FAULT_WIDGET_PATH,
    CONTROLLER_FAULT_PIN,
    HOME_ALL_OPERATION_ID,
    HOME_ALL_WIDGET_PATH,
    HOMING_STATE_POLL_MILLISECONDS,
    POSITION_KNOWN_PIN,
    POSITION_UNKNOWN_PIN,
)
from .operation_catalog import project_catalog_path, read_operations


class HomingSectionBinding:
    """Keep the always-present base controls and their indicators current."""

    def __init__(
        self,
        *,
        component,
        root_window,
        known_indicator_path: str,
        unknown_indicator_path: str,
        operation,
        clear_fault_operation,
        clear_fault_command,
        clear_fault_widget_path: str,
    ) -> None:
        self.component = component
        self.root_window = root_window
        self.known_indicator_path = known_indicator_path
        self.unknown_indicator_path = unknown_indicator_path
        self.operation = operation
        self.clear_fault_operation = clear_fault_operation
        self.clear_fault_command = clear_fault_command
        self.clear_fault_widget_path = clear_fault_widget_path
        self.poll_after_id = None

    def _pin(self, name: str) -> bool:
        try:
            return bool(self.component[name])
        except (KeyError, RuntimeError):
            return False

    def poll(self) -> None:
        """Refresh the base-control indicators without restricting their commands."""
        self.root_window.tk.call(
            self.known_indicator_path,
            "itemconfigure",
            "led",
            "-fill",
            "#00a000" if self._pin(POSITION_KNOWN_PIN) else "#595959",
        )
        self.root_window.tk.call(
            self.unknown_indicator_path,
            "itemconfigure",
            "led",
            "-fill",
            "#d00000" if self._pin(POSITION_UNKNOWN_PIN) else "#595959",
        )
        self.root_window.tk.call(
            self.clear_fault_widget_path,
            "configure",
            "-background",
            "#d00000" if self._pin(CONTROLLER_FAULT_PIN) else "#d9d9d9",
            "-activebackground",
            "#ef3030" if self._pin(CONTROLLER_FAULT_PIN) else "#ececec",
        )
        self.poll_after_id = self.root_window.after(
            HOMING_STATE_POLL_MILLISECONDS,
            self.poll,
        )

    def request_fault_clear(self) -> None:
        """Issue only LinuxCNC's canonical E-stop-reset state request."""
        self.clear_fault_command()


def _create_indicator(tk, *, path: str) -> None:
    tk.call(
        "canvas",
        path,
        "-width",
        14,
        "-height",
        14,
        "-borderwidth",
        0,
        "-highlightthickness",
        0,
    )
    tk.call(
        path,
        "create",
        "oval",
        2,
        2,
        12,
        12,
        "-fill",
        "#595959",
        "-outline",
        "#303030",
        "-tags",
        "led",
    )


def install_axis_base_controls(namespace: Mapping[str, object]) -> HomingSectionBinding:
    """Move Home All and homing state into one Manual-tab section."""
    live_plotter = namespace["live_plotter"]
    existing = getattr(live_plotter, "_dmc2_base_controls", None)
    if existing is not None:
        return existing

    operations = read_operations(project_catalog_path(str(namespace["rcfile"])))
    operation = operations.get(HOME_ALL_OPERATION_ID)
    if operation is None:
        raise RuntimeError(f"Operation is missing: {HOME_ALL_OPERATION_ID}")
    if (
        operation.kind != "control"
        or operation.driver != "linuxcnc.home-all"
        or operation.ui_scope != "manual-tab-homing"
    ):
        raise RuntimeError(f"Operation has invalid Home All contract: {operation!r}")
    clear_fault_operation = operations.get(CLEAR_FAULT_OPERATION_ID)
    if clear_fault_operation is None:
        raise RuntimeError(f"Operation is missing: {CLEAR_FAULT_OPERATION_ID}")
    if (
        clear_fault_operation.kind != "control"
        or clear_fault_operation.driver != "linuxcnc.task-state"
        or clear_fault_operation.target != "estop-reset"
        or clear_fault_operation.ui_scope != "base-toolbar"
    ):
        raise RuntimeError(
            f"Operation has invalid Clear Fault contract: {clear_fault_operation!r}"
        )

    component = namespace["comp"]
    hal_module = namespace["hal"]
    component.newpin(POSITION_KNOWN_PIN, hal_module.HAL_BIT, hal_module.HAL_IN)
    component.newpin(POSITION_UNKNOWN_PIN, hal_module.HAL_BIT, hal_module.HAL_IN)
    component.newpin(CONTROLLER_FAULT_PIN, hal_module.HAL_BIT, hal_module.HAL_IN)

    root_window = namespace["root_window"]
    tk = root_window.tk
    tabs_manual = str(namespace["tabs_manual"])
    section_path = f"{tabs_manual}.dmc2_homing"
    section_label_path = f"{tabs_manual}.dmc2_homing_label"
    home_widget_path = f"{section_path}.home_all"
    known_indicator_path = f"{section_path}.known_led"
    unknown_indicator_path = f"{section_path}.unknown_led"
    if home_widget_path != HOME_ALL_WIDGET_PATH:
        raise RuntimeError(f"Unexpected AXIS Manual-tab path: {home_widget_path!r}")

    linuxcnc_module = namespace["linuxcnc"]
    command_channel = namespace["c"]

    def clear_fault_command() -> None:
        command_channel.state(linuxcnc_module.STATE_ESTOP_RESET)

    stock_home_button = namespace["widgets"].homebutton
    stock_home_path = str(stock_home_button)
    stock_home_command = str(stock_home_button.cget("command"))
    if not stock_home_command:
        raise RuntimeError("AXIS's stock Home All command is unavailable")

    legacy_toolbar_path = ".toolbar.dmc2_home_all"
    if int(tk.call("winfo", "exists", legacy_toolbar_path)):
        tk.call("destroy", legacy_toolbar_path)

    tk.call("label", section_label_path, "-text", "Homing:", "-anchor", "nw")
    tk.call("frame", section_path)
    tk.call(
        "button",
        home_widget_path,
        "-command",
        stock_home_command,
        "-text",
        operation.label,
        "-padx",
        "2m",
        "-pady",
        0,
        "-state",
        str(stock_home_button.cget("state")),
    )
    _create_indicator(tk, path=known_indicator_path)
    tk.call(
        "label",
        f"{section_path}.known_label",
        "-text",
        "Known",
        "-anchor",
        "w",
    )
    _create_indicator(tk, path=unknown_indicator_path)
    tk.call(
        "label",
        f"{section_path}.unknown_label",
        "-text",
        "Unknown",
        "-anchor",
        "w",
    )

    tk.call("pack", home_widget_path, "-side", "left", "-padx", 2, "-pady", 2)
    tk.call(
        "pack",
        known_indicator_path,
        "-side",
        "left",
        "-padx",
        3,
    )
    tk.call("pack", f"{section_path}.known_label", "-side", "left")
    tk.call(
        "pack",
        unknown_indicator_path,
        "-side",
        "left",
        "-padx",
        3,
    )
    tk.call("pack", f"{section_path}.unknown_label", "-side", "left")

    tk.call("grid", "remove", stock_home_path)
    tk.call(
        "grid",
        "configure",
        f"{tabs_manual}.jogf.zerohome.zero",
        "-column",
        0,
    )
    tk.call("grid", "remove", f"{tabs_manual}.space2")
    tk.call(
        "grid",
        section_label_path,
        "-column",
        0,
        "-row",
        4,
        "-pady",
        2,
        "-sticky",
        "nw",
    )
    tk.call(
        "grid",
        section_path,
        "-column",
        1,
        "-row",
        4,
        "-columnspan",
        2,
        "-padx",
        2,
        "-sticky",
        "w",
    )
    tk.call("lappend", "manualgroup", home_widget_path)
    tk.call(
        "DynamicHelp::add",
        home_widget_path,
        "-text",
        "Home all axes [Ctrl-Home]",
    )

    binding = HomingSectionBinding(
        component=component,
        root_window=root_window,
        known_indicator_path=known_indicator_path,
        unknown_indicator_path=unknown_indicator_path,
        operation=operation,
        clear_fault_operation=clear_fault_operation,
        clear_fault_command=clear_fault_command,
        clear_fault_widget_path=CLEAR_FAULT_WIDGET_PATH,
    )
    clear_fault_tcl_command = root_window.register(binding.request_fault_clear)
    tk.call(
        "button",
        CLEAR_FAULT_WIDGET_PATH,
        "-command",
        clear_fault_tcl_command,
        "-text",
        clear_fault_operation.label.upper(),
        "-padx",
        "2m",
        "-pady",
        0,
        "-takefocus",
        0,
    )
    tk.call(
        "pack",
        CLEAR_FAULT_WIDGET_PATH,
        "-side",
        "left",
        "-after",
        ".toolbar.machine_estop",
        "-padx",
        2,
    )
    tk.call(
        "DynamicHelp::add",
        CLEAR_FAULT_WIDGET_PATH,
        "-text",
        "Clear the retained controller fault through LinuxCNC E-stop Reset",
    )
    binding.poll()
    live_plotter._dmc2_base_controls = binding
    return binding
