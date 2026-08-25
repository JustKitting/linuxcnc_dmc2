from __future__ import annotations

import types
import unittest
from pathlib import Path

from axis_ui_policy import (
    CONTROLLER_AVAILABLE_PIN,
    CONTROLLER_READY_PIN,
    EXPECTED_LIMIT_STOP_MESSAGE,
    PENDANT_MODE_PIN,
    PENDANT_WIDGET_PATH,
    PendantModeBinding,
    install_axis_pendant_mode,
    install_axis_ui_policy,
)


class FakeErrorChannel:
    def __init__(self, errors):
        self.errors = list(errors)

    def poll(self):
        return self.errors.pop(0) if self.errors else None


class FakeNotifications:
    def __init__(self):
        self.shown = []
        self.placements = []

    def add(self, icon, message):
        self.shown.append((icon, message))

    def place_configure(self, **kwargs):
        self.placements.append(kwargs)


class FakeWindow:
    def __init__(self):
        self.scheduled = []

    def after(self, milliseconds, callback):
        self.scheduled.append((milliseconds, callback))
        return "scheduled-id"


class FakeVariable:
    def __init__(self, value=True):
        self.value = value

    def get(self):
        return self.value

    def set(self, value):
        self.value = bool(value)


class FakeComponent(dict):
    def __init__(self):
        super().__init__()
        self.created_pins = []

    def newpin(self, name, pin_type, direction):
        self.created_pins.append((name, pin_type, direction))
        self[name] = False


class FakeTclError(Exception):
    pass


class FakeTk:
    def __init__(self):
        self.calls = []

    def call(self, *arguments):
        self.calls.append(arguments)
        if arguments == (".menu.view", "index", "end"):
            return 4
        if arguments[:2] == (".menu.view", "entrycget"):
            if arguments[2] == 2:
                return "show_pyvcppanel"
            raise FakeTclError("entry has no variable")
        return ""


class FakeRoot:
    def __init__(self):
        self.tk = FakeTk()
        self.registered = []
        self.bindings = {}
        self.scheduled = []

    def register(self, callback):
        self.registered.append(callback)
        return "dmc2-toggle-command"

    def bind(self, event, callback):
        self.bindings[event] = callback

    def after(self, milliseconds, callback):
        self.scheduled.append((milliseconds, callback))
        return f"after-{len(self.scheduled)}"


class FakeBitmapImage:
    created = []

    def __init__(self, **options):
        self.options = options
        self.name = f"fake-image-{len(self.created)}"
        self.created.append(self)

    def __str__(self):
        return self.name


class AxisUiPolicyTests(unittest.TestCase):
    def make_namespace(self, errors):
        linuxcnc = types.SimpleNamespace(NML_ERROR=1, OPERATOR_ERROR=2)
        notifications = FakeNotifications()
        live_plotter = types.SimpleNamespace(
            win=FakeWindow(),
            error_after=None,
            error_task=lambda: None,
        )
        namespace = {
            "e": FakeErrorChannel(errors),
            "linuxcnc": linuxcnc,
            "notifications": notifications,
            "live_plotter": live_plotter,
        }
        return namespace, notifications, live_plotter

    def test_only_exact_expected_limit_stop_errors_are_suppressed(self):
        errors = [
            (1, EXPECTED_LIMIT_STOP_MESSAGE),
            (1, "Joint 0 following error"),
            (2, EXPECTED_LIMIT_STOP_MESSAGE),
            (99, "informational message"),
        ]
        namespace, notifications, live_plotter = self.make_namespace(errors)

        install_axis_ui_policy(namespace)
        live_plotter.error_task()

        self.assertEqual(
            notifications.shown,
            [
                ("error", "Joint 0 following error"),
                ("info", "informational message"),
            ],
        )
        self.assertEqual(live_plotter.error_after, "scheduled-id")
        self.assertEqual(len(live_plotter.win.scheduled), 1)

    def test_visible_notifications_are_moved_away_from_status_panel(self):
        namespace, notifications, live_plotter = self.make_namespace(
            [(1, "actionable fault")]
        )
        install_axis_ui_policy(namespace)
        live_plotter.error_task()

        self.assertEqual(
            notifications.placements,
            [{"relx": 0, "rely": 1, "x": 20, "y": -20, "anchor": "sw"}],
        )

    def test_axis_user_command_installs_the_same_policy(self):
        namespace, _notifications, live_plotter = self.make_namespace([])
        command_file = Path(__file__).with_name("axis_user_command.py")
        namespace["rcfile"] = str(command_file)
        namespace["__builtins__"] = __builtins__

        exec(compile(command_file.read_bytes(), str(command_file), "exec"), namespace)

        self.assertTrue(live_plotter._dmc2_ui_policy_installed)
        self.assertTrue(callable(namespace["user_hal_pins"]))

    def make_pendant_namespace(self):
        FakeBitmapImage.created = []
        component = FakeComponent()
        root = FakeRoot()
        panel_variable = FakeVariable(True)
        panel_calls = []

        def toggle_panel():
            panel_calls.append(panel_variable.get())

        namespace = {
            "comp": component,
            "hal": types.SimpleNamespace(
                HAL_BIT="bit",
                HAL_IN="in",
                HAL_OUT="out",
            ),
            "root_window": root,
            "Tkinter": types.SimpleNamespace(
                BitmapImage=FakeBitmapImage,
                TclError=FakeTclError,
            ),
            "vars": types.SimpleNamespace(show_pyvcppanel=panel_variable),
            "commands": types.SimpleNamespace(toggle_show_pyvcppanel=toggle_panel),
            "live_plotter": types.SimpleNamespace(),
            "rcfile": str(Path(__file__).with_name("axis_user_command.py")),
        }
        return namespace, component, root, panel_variable, panel_calls

    def test_pendant_toolbar_is_packed_immediately_after_clear_plot(self):
        namespace, component, root, panel_variable, panel_calls = (
            self.make_pendant_namespace()
        )

        binding = install_axis_pendant_mode(namespace)

        self.assertEqual(
            component.created_pins,
            [
                (PENDANT_MODE_PIN, "bit", "out"),
                (CONTROLLER_AVAILABLE_PIN, "bit", "in"),
                (CONTROLLER_READY_PIN, "bit", "in"),
            ],
        )
        self.assertFalse(component[PENDANT_MODE_PIN])
        self.assertFalse(binding.enabled)
        self.assertFalse(panel_variable.get())
        self.assertEqual(panel_calls, [False])
        self.assertIn(
            (
                "pack",
                PENDANT_WIDGET_PATH,
                "-side",
                "left",
                "-after",
                ".toolbar.clear_plot",
            ),
            root.tk.calls,
        )
        self.assertEqual(root.bindings["<Control-e>"], binding.toggle)
        self.assertIn(
            (
                ".menu.view",
                "entryconfigure",
                2,
                "-command",
                "dmc2-toggle-command",
            ),
            root.tk.calls,
        )
        self.assertEqual(len(FakeBitmapImage.created), 2)
        self.assertEqual(len(root.scheduled), 1)
        self.assertTrue(
            all(
                image.options["file"].endswith("pendant_icon.xbm")
                for image in FakeBitmapImage.created
            )
        )

    def test_axis_user_hal_hook_installs_the_toolbar_and_pin(self):
        namespace, component, root, _panel_variable, _panel_calls = (
            self.make_pendant_namespace()
        )
        notification_namespace, _notifications, _live_plotter = self.make_namespace([])
        namespace.update(notification_namespace)
        namespace["root_window"] = root
        namespace["comp"] = component
        namespace["rcfile"] = str(Path(__file__).with_name("axis_user_command.py"))
        namespace["__builtins__"] = __builtins__

        command_file = Path(__file__).with_name("axis_user_command.py")
        exec(compile(command_file.read_bytes(), str(command_file), "exec"), namespace)
        namespace["user_hal_pins"]()

        self.assertEqual(
            component.created_pins,
            [
                (PENDANT_MODE_PIN, "bit", "out"),
                (CONTROLLER_AVAILABLE_PIN, "bit", "in"),
                (CONTROLLER_READY_PIN, "bit", "in"),
            ],
        )
        self.assertFalse(component[PENDANT_MODE_PIN])
        self.assertIn(
            ("pack", PENDANT_WIDGET_PATH, "-side", "left", "-after", ".toolbar.clear_plot"),
            root.tk.calls,
        )

    def test_toolbar_toggle_arms_shows_then_disarms_hides(self):
        namespace, component, root, panel_variable, panel_calls = (
            self.make_pendant_namespace()
        )
        binding = install_axis_pendant_mode(namespace)
        panel_calls.clear()

        component[CONTROLLER_AVAILABLE_PIN] = True
        binding.synchronize_readiness()

        binding.toggle()
        self.assertTrue(binding.requested)
        self.assertFalse(binding.enabled)
        self.assertTrue(component[PENDANT_MODE_PIN])
        self.assertFalse(panel_variable.get())
        self.assertEqual(panel_calls, [])

        component[CONTROLLER_READY_PIN] = True
        binding.synchronize_readiness()
        self.assertTrue(binding.enabled)
        self.assertTrue(component[PENDANT_MODE_PIN])
        self.assertTrue(panel_variable.get())
        self.assertEqual(panel_calls, [True])
        self.assertIn(
            (
                PENDANT_WIDGET_PATH,
                "configure",
                "-image",
                str(binding.active_image),
                "-relief",
                "sunken",
                "-state",
                "normal",
            ),
            root.tk.calls,
        )

        binding.toggle()
        self.assertFalse(binding.enabled)
        self.assertFalse(binding.requested)
        self.assertFalse(component[PENDANT_MODE_PIN])
        self.assertFalse(panel_variable.get())
        self.assertEqual(panel_calls, [True, False])

    def test_failed_panel_show_never_arms_hal(self):
        component = {
            PENDANT_MODE_PIN: False,
            CONTROLLER_AVAILABLE_PIN: True,
            CONTROLLER_READY_PIN: True,
        }
        variable = FakeVariable(False)
        root = FakeRoot()

        def fail_to_show():
            raise RuntimeError("panel failed")

        binding = PendantModeBinding(
            component=component,
            show_panel_variable=variable,
            toggle_panel=fail_to_show,
            root_window=root,
            tk=root.tk,
            widget_path=PENDANT_WIDGET_PATH,
            inactive_image="inactive",
            active_image="active",
        )

        with self.assertRaisesRegex(RuntimeError, "panel failed"):
            binding.set_enabled(True)
        self.assertFalse(component[PENDANT_MODE_PIN])
        self.assertFalse(variable.get())
        self.assertFalse(binding.enabled)

    def test_unavailable_controller_cannot_arm_or_show_panel(self):
        namespace, component, _root, panel_variable, panel_calls = (
            self.make_pendant_namespace()
        )
        binding = install_axis_pendant_mode(namespace)
        panel_calls.clear()

        self.assertFalse(binding.set_enabled(True))
        self.assertFalse(binding.requested)
        self.assertFalse(binding.enabled)
        self.assertFalse(component[PENDANT_MODE_PIN])
        self.assertFalse(panel_variable.get())
        self.assertEqual(panel_calls, [])

    def test_temporary_not_ready_state_keeps_available_panel_armed_and_visible(self):
        namespace, component, _root, panel_variable, panel_calls = (
            self.make_pendant_namespace()
        )
        binding = install_axis_pendant_mode(namespace)
        panel_calls.clear()
        component[CONTROLLER_AVAILABLE_PIN] = True
        binding.synchronize_readiness()
        binding.set_enabled(True)
        component[CONTROLLER_READY_PIN] = True
        binding.synchronize_readiness()
        self.assertTrue(binding.enabled)
        self.assertTrue(panel_variable.get())

        component[CONTROLLER_READY_PIN] = False
        binding.synchronize_readiness()

        self.assertTrue(component[PENDANT_MODE_PIN])
        self.assertTrue(binding.requested)
        self.assertTrue(binding.enabled)
        self.assertTrue(panel_variable.get())
        self.assertEqual(panel_calls, [True])

    def test_controller_unavailable_disarms_and_hides_visible_panel(self):
        namespace, component, _root, panel_variable, panel_calls = (
            self.make_pendant_namespace()
        )
        binding = install_axis_pendant_mode(namespace)
        panel_calls.clear()
        component[CONTROLLER_AVAILABLE_PIN] = True
        binding.synchronize_readiness()
        binding.set_enabled(True)
        component[CONTROLLER_READY_PIN] = True
        binding.synchronize_readiness()
        self.assertTrue(binding.enabled)
        self.assertTrue(panel_variable.get())

        component[CONTROLLER_READY_PIN] = False
        component[CONTROLLER_AVAILABLE_PIN] = False
        binding.synchronize_readiness()

        self.assertFalse(component[PENDANT_MODE_PIN])
        self.assertFalse(binding.requested)
        self.assertFalse(binding.enabled)
        self.assertFalse(panel_variable.get())
        self.assertEqual(panel_calls, [True, False])


if __name__ == "__main__":
    unittest.main()
