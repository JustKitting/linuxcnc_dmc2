"""AXIS pane bindings; execute unchanged files through the existing typed loader."""

from __future__ import annotations

from pathlib import Path
from .block_plan import create_plan_parameters
from .calibration_data import create_calibration_parameters

from .constants import CUSTOM_SCRIPTS_CONTENT, GO_TO_HOME_OPERATION_ID, GO_TO_HOME_WIDGET_PATH, HOMING_STATE_POLL_MILLISECONDS, PROBE_SECTION_PATH
from .operation_catalog import project_catalog_path, read_operations
from .recovery_ui import RecoveryUiNotice
from .script_contract import ScriptInspector
from .script_panel_layout import create_home_widgets, create_pane, create_script_widgets
from .script_panel_model import read_panel_scripts
from .ui_fault import AxisUiFault, AxisUiFaultKind


class CustomScriptsBinding:
    def __init__(self, namespace):
        self.ns = namespace
        self.root = namespace["root_window"]
        self.tk = self.root.tk
        self.comp = namespace["comp"]
        self.project = Path(str(namespace["rcfile"])).resolve().parents[2]
        self.refresh_notice = RecoveryUiNotice(namespace)
        self.run_notice = RecoveryUiNotice(namespace)
        self.preference_notices = {}
        self.publish_notices = {}
        self.variables = {}
        self.widgets = {}
        self.parameter_errors = {}
        self.contracts = {}
        self.paths = {}
        self.locked = False
        self.pending_serial = None
        self.after_id = None
        operations = read_operations(project_catalog_path(str(namespace["rcfile"])))
        self.scripts = {item.key: item for item in read_panel_scripts(self.project / "config/script-panel.json", operations)}
        home = operations[GO_TO_HOME_OPERATION_ID]
        if home.ui_target != GO_TO_HOME_WIDGET_PATH or home.ui_scope != "custom-scripts":
            raise ValueError("Go to Home does not point to its Custom Scripts control.")
        self.operations = {key: script.operation for key, script in self.scripts.items()}
        self.operations["home"] = home
        inspector = ScriptInspector(self.project / "native/bin/dmc2ctl", self.project)
        for key, operation in self.operations.items():
            path = (self.project / operation.target).resolve()
            self.paths[key] = path
            self.contracts[key] = inspector.inspect(path)
        hal = namespace["hal"]
        create_plan_parameters(self.comp, hal, self.project / "config/probe-plan-banks.tsv")
        create_calibration_parameters(self.comp, hal, self.project / "native/bin/dmc2-probe-capture")
        for script in self.scripts.values():
            self.preference_notices[script.key] = RecoveryUiNotice(namespace)
            self.publish_notices[script.key] = RecoveryUiNotice(namespace)
            self.comp.newpin(script.valid_pin, hal.HAL_BIT, hal.HAL_OUT)
            self.comp[script.valid_pin] = False
            for parameter in script.parameters:
                self.comp.newpin(parameter.pin, hal.HAL_FLOAT, hal.HAL_OUT)
                variable = namespace["Tkinter"].StringVar(master=self.root)
                preferred = namespace["ap"].getpref("dmc2_" + parameter.pin, parameter.default, str)
                variable.set(parameter.initial_text(preferred))
                self.variables[parameter.pin] = variable
            self.widgets[script.key] = create_script_widgets(self, script)
            for parameter in script.parameters:
                self.variables[parameter.pin].trace_add("write", lambda *_, key=script.key: self.apply_parameters(key))
            self.apply_parameters(script.key)
        # A menu selects the visible form without changing machine mode.
        menu = CUSTOM_SCRIPTS_CONTENT + ".choose"
        self.selected = namespace["Tkinter"].StringVar(master=self.root, value=next(iter(self.scripts)))
        self.tk.call("menubutton", menu, "-text", self.scripts[self.selected.get()].operation.label, "-relief", "raised", "-menu", menu + ".menu")
        self.tk.call("menu", menu + ".menu", "-tearoff", 0)
        for key, script in self.scripts.items():
            self.tk.call(menu + ".menu", "add", "command", "-label", script.operation.label,
                         "-command", self.root.register(lambda key=key: self.select(key)))
        self.tk.call("pack", menu, "-fill", "x", "-before", PROBE_SECTION_PATH)
        self.widgets["home"] = create_home_widgets(self, home)
        self.select(self.selected.get())

    def select(self, key):
        self.selected.set(key)
        self.tk.call(CUSTOM_SCRIPTS_CONTENT + ".choose", "configure", "-text", self.scripts[key].operation.label)
        for name in self.scripts:
            self.tk.call("pack", "forget", self.widgets[name]["frame"])
        self.tk.call("pack", self.widgets[key]["frame"], "-fill", "x", "-pady", 4,
                     "-before", CUSTOM_SCRIPTS_CONTENT + ".home")

    def texts(self, key):
        return {field.pin: self.variables[field.pin].get() for field in self.scripts[key].parameters}

    def apply_parameters(self, key):
        if self.locked:
            return
        script = self.scripts[key]
        try:
            # Invalid editing text cannot leave an old value looking usable.
            self.comp[script.valid_pin] = False
            values = script.values(self.texts(key))
            for pin, value in values.items():
                self.comp[pin] = value
            self.comp[script.valid_pin] = True
        except ValueError as error:
            self.parameter_errors[key] = str(error)
        except Exception as error:
            self.parameter_errors[key] = f"Could not apply parameters: {error}. Edit the field to retry."
            self.publish_notices[key].present(fault=AxisUiFault(AxisUiFaultKind.CUSTOM_SCRIPTS_REFRESH_FAILED, error))
        else:
            self.parameter_errors.pop(key, None)
            self.publish_notices[key].clear()

    def save_preferences(self, key):
        if key in self.parameter_errors:
            return
        try:
            for pin, text in self.texts(key).items():
                self.ns["ap"].putpref("dmc2_" + pin, text, str)
        except Exception as error:
            self.preference_notices[key].present(fault=AxisUiFault(AxisUiFaultKind.CUSTOM_SCRIPTS_PREFERENCES_FAILED, error))
        else:
            self.preference_notices[key].clear()

    def increment(self, key, parameter, direction):
        if self.locked:
            return
        try:
            self.variables[parameter.pin].set(parameter.incremented(self.variables[parameter.pin].get(), direction))
            self.save_preferences(key)
        except ValueError as error:
            self.parameter_errors[key] = str(error)

    def key_for_path(self, path):
        if not path:
            return None
        resolved = Path(path).resolve()
        return next((key for key, source in self.paths.items() if source == resolved), None)

    def parameter_issue(self, path):
        key = self.key_for_path(path)
        if key is not None and self.locked:
            return "A script submission is pending. Wait for it to stop or use the visible Abort control."
        return self.parameter_errors.get(key)

    def prepare_for_run(self, path):
        key = self.key_for_path(path)
        if key is None:
            return False
        if self.locked:
            raise ValueError("A script submission is pending. Wait for it to stop or use the visible Abort control.")
        if key in self.scripts:
            if self.scripts[key].requires_beginning and int(self.ns.get("program_start_line", 0)) != 0:
                raise ValueError("This scan must establish a fresh top reference. Use its Run button in Custom Scripts to reopen it from the beginning.")
            self.apply_parameters(key)
            if issue := self.parameter_errors.get(key):
                raise ValueError(issue + " Open Custom Scripts to correct it, then retry Run.")
            self.save_preferences(key)
        self.set_editable(False)
        self.locked = True
        self.pending_serial = None
        return True

    def finish_submission(self, previous_serial):
        current = int(self.ns["c"].serial)
        if current == previous_serial:
            self.cancel_preparation()
        else:
            self.pending_serial = current

    def cancel_preparation(self):
        """Release a UI lock when no command has been submitted."""
        self.locked = False
        self.pending_serial = None

    def set_editable(self, enabled):
        for key in self.scripts:
            for widget in self.widgets[key]["inputs"]:
                self.tk.call(widget, "configure", "-state", "normal" if enabled else "disabled")

    def run(self, key):
        try:
            guard = self.ns["live_plotter"]._dmc2_axis_run_guard
            snapshot = guard._status_snapshot()
            reason = self.parameter_issue(self.paths[key]) or guard.readiness_message(self.contracts[key].prerequisites, snapshot)
            if reason:
                self.tk.call(self.widgets[key]["status"], "configure", "-text", reason)
                return
            commands = self.ns["commands"]
            commands.open_file_name(str(self.paths[key]))
            loader = self.ns["live_plotter"]._dmc2_axis_script_loader
            if loader.contract_for_loaded_path(self.paths[key]) is None:
                return
            commands.task_run()
        except Exception as error:
            self.run_notice.present(fault=AxisUiFault(AxisUiFaultKind.PROGRAM_RUN_SUBMISSION_FAILED, error))
        else:
            self.run_notice.clear()

    def poll(self):
        try:
            guard = self.ns["live_plotter"]._dmc2_axis_run_guard
            snapshot = guard._status_snapshot()
            status = self.ns["s"]
            linuxcnc = self.ns["linuxcnc"]
            idle = snapshot.interpreter_state == int(linuxcnc.INTERP_IDLE)
            if self.locked and self.pending_serial is not None and idle:
                if int(status.echo_serial_number) >= self.pending_serial and int(status.state) in (int(linuxcnc.RCS_DONE), int(linuxcnc.RCS_ERROR)):
                    self.locked = False
                    self.pending_serial = None
            self.set_editable(idle and not self.locked)
            for key in self.operations:
                reason = self.parameter_errors.get(key) or guard.readiness_message(self.contracts[key].prerequisites, snapshot)
                if self.locked:
                    reason = "Program submitted. Settings unlock when it stops; Abort remains available."
                self.tk.call(self.widgets[key]["run"], "configure", "-state", "disabled" if reason else "normal")
                ready = "Ready. Run uses the values above." if key != "home" else "Ready to return to the established machine home."
                self.tk.call(self.widgets[key]["status"], "configure", "-text", reason or ready)
        except Exception as error:
            presentation_errors = []
            for widgets in self.widgets.values():
                try:
                    self.tk.call(widgets["run"], "configure", "-state", "disabled")
                except Exception as widget_error:
                    presentation_errors.append(str(widget_error))
            self.refresh_notice.present(fault=AxisUiFault(AxisUiFaultKind.CUSTOM_SCRIPTS_REFRESH_FAILED, f"{error}; control display failures={presentation_errors}"))
        else:
            self.refresh_notice.clear()
        finally:
            try:
                self.after_id = self.root.after(HOMING_STATE_POLL_MILLISECONDS, self.poll)
            except Exception as error:
                self.refresh_notice.present(fault=AxisUiFault(AxisUiFaultKind.CUSTOM_SCRIPTS_INSTALL_FAILED, f"Display refresh could not be scheduled: {error}"))


def install_axis_custom_scripts(namespace):
    create_pane(namespace)
    binding = CustomScriptsBinding(namespace)
    namespace["live_plotter"]._dmc2_custom_scripts = binding
    binding.poll()
    return binding
