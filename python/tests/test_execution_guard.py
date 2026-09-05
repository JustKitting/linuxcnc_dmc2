"""Non-hardware failure checks for the production AXIS execution boundary."""

import contextlib
import io
from pathlib import Path
import re
import runpy
import tempfile
import tkinter
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from dmc2_axis import run_guard
from dmc2_axis.script_contract import CONSERVATIVE_PREREQUISITES


ENTRY = Path(__file__).resolve().parents[1] / "dmc2_axis" / "axis_user_command.py"


class ExecutionGuardTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="dmc2-axis-guard-")
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / "job.ngc"
        self.path.write_text("M2\n")
        self.calls = []
        self.faults = []
        self.contract = SimpleNamespace(
            prerequisites=CONSERVATIVE_PREREQUISITES,
            source=SimpleNamespace(value="conservative-default"),
        )
        self.loader = SimpleNamespace(
            inspect_for_run=lambda path: self.contract,
            contract_for_loaded_path=lambda path: self.contract,
        )
        self.status = SimpleNamespace(
            poll=lambda: None, joints=3, homed=[True] * 3,
            interp_state=1, task_state=4, task_mode=1,
            estop=False, enabled=True, file=str(self.path),
        )
        self.namespace = {
            "loaded_file": str(self.path),
            "live_plotter": SimpleNamespace(_dmc2_axis_script_loader=self.loader),
        }
        self.guard = run_guard.AxisRunGuard(
            namespace=self.namespace, status=self.status,
            linuxcnc_module=SimpleNamespace(INTERP_IDLE=1, STATE_ESTOP=1, STATE_ON=4, MODE_AUTO=2),
            stock_commands={
                name: lambda *args, name=name: self.calls.append(name)
                for name in ("task_run", "task_step")
            },
        )
        self.guard._present = lambda *, kind, cause: self.faults.append(kind)

    def submit(self, name):
        with contextlib.redirect_stdout(io.StringIO()):
            return self.guard.submit(name)

    def test_initial_step_and_run_both_require_declared_homing(self):
        self.status.homed[1] = False
        for name in ("task_run", "task_step"):
            self.assertEqual(self.submit(name), "break")
        self.assertEqual(self.calls, [])
        self.status.homed[1] = True
        self.submit("task_step")
        self.assertEqual(self.calls, ["task_step"])

    def test_active_auto_step_does_not_require_idle_but_run_does(self):
        self.status.interp_state = 3
        self.status.task_mode = 2
        self.submit("task_step")
        self.assertEqual(self.submit("task_run"), "break")
        self.assertEqual(self.calls, ["task_step"])

    def test_failed_inspection_and_missing_loader_block_step(self):
        self.contract = None
        self.assertEqual(self.submit("task_step"), "break")
        del self.namespace["live_plotter"]._dmc2_axis_script_loader
        self.assertEqual(self.submit("task_step"), "break")
        self.assertEqual(self.calls, [])


class BootstrapInterlockTests(unittest.TestCase):
    def test_partial_installations_never_restore_stock_execution(self):
        for failure in ("loader", "guard", "tcl-step", "key-step"):
            with self.subTest(failure=failure):
                self.check_failure(failure)

    def check_failure(self, failure):
        root = tkinter.Tcl()  # No Tk window, LinuxCNC connection, or HAL component.
        bindings = {}
        root.tk.createcommand("bind", lambda widget, key, *value:
            bindings.setdefault((widget, key), "") if not value
            else bindings.__setitem__((widget, key), value[0]))
        calls = []
        commands = SimpleNamespace(**{
            name: lambda *args, name=name: calls.append(name)
            for name in ("task_run", "task_step", "task_stop")
        })
        for name, key in (("task_run", "r"), ("task_step", "t")):
            root.tk.createcommand(name, getattr(commands, name))
            root.bind(key, getattr(commands, name))
        original_abort = commands.task_stop
        installed = []

        class TkProxy:
            def __getattr__(self, name):
                return getattr(root.tk, name)

            def createcommand(self, name, callback):
                if failure == "tcl-step" and name == "task_step":
                    raise RuntimeError("injected Tcl registration failure")
                return root.tk.createcommand(name, callback)

        def bind(key, callback):
            if failure == "key-step" and key == "t":
                raise RuntimeError("injected keyboard registration failure")
            return root.bind(key, callback)

        def import_extension(name, package):
            if (failure == "loader" and name == ".script_loader") or (
                failure == "guard" and name == ".run_guard"
            ):
                raise ImportError(f"injected {name} import failure")
            if name == ".run_guard":
                return run_guard
            installer_names = {
                ".notifications": "install_axis_ui_policy",
                ".script_loader": "install_axis_script_loader",
                ".base_controls": "install_axis_base_controls",
                ".pendant_mode": "install_axis_pendant_mode",
                ".spindle_feedback": "install_axis_spindle_feedback",
            }

            def install(namespace):
                installed.append(name)
                if name == ".script_loader":
                    namespace["live_plotter"]._dmc2_axis_script_loader = object()
                return object()

            return SimpleNamespace(**{installer_names[name]: install})

        namespace = {
            "rcfile": str(ENTRY), "commands": commands,
            "root_window": SimpleNamespace(tk=TkProxy(), _w=root._w, bind=bind),
            "live_plotter": SimpleNamespace(), "s": object(), "linuxcnc": object(),
            "notifications": SimpleNamespace(add=lambda *args: None),
        }
        with patch("importlib.import_module", side_effect=import_extension), contextlib.redirect_stdout(io.StringIO()):
            result = runpy.run_path(str(ENTRY), init_globals=namespace)
            result["user_hal_pins"]()
            self.assertIs(commands.task_stop, original_abort)
            self.assertIn(".base_controls", installed)
            self.assertIn(".pendant_mode", installed)
            for name, key in (("task_run", "r"), ("task_step", "t")):
                self.assertEqual(getattr(commands, name)(), "break")
                if str(root.tk.call("info", "commands", name)):
                    self.assertEqual(root.tk.call(name), "break")
                binding = bindings.get((root._w, key), "")
                if binding:
                    callback_name = re.search(r"\[([^\s]+)", binding).group(1)
                    self.assertEqual(root.tk.call(callback_name), "break")
        self.assertEqual(calls, [])


if __name__ == "__main__":
    unittest.main()
