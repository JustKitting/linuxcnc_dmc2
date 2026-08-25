from __future__ import annotations

import types
import unittest
from contextlib import redirect_stdout
from io import StringIO
from unittest.mock import patch

from tests.python._support import AXIS_COMMAND_FILE

import dmc2_axis.notifications as notification_policy

from dmc2_axis import (
    CONTROLLER_AVAILABLE_PIN,
    CONTROLLER_READY_PIN,
    EXPECTED_LIMIT_STOP_MESSAGE,
    PENDANT_MODE_PIN,
    PENDANT_WIDGET_PATH,
    PendantModeBinding,
    error_channel_kind_catalog,
    install_axis_pendant_mode,
    install_axis_ui_policy,
)


class FakeErrorChannel:
    def __init__(self, errors):
        self.errors = list(errors)

    def poll(self):
        if not self.errors:
            return None
        result = self.errors.pop(0)
        if isinstance(result, BaseException):
            raise result
        return result


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


class UnstringableMessage:
    def __str__(self):
        raise RuntimeError("message conversion failed")

    def __repr__(self):
        return "UnstringableMessage()"


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
        linuxcnc = types.SimpleNamespace(
            version="2.9.10",
            NML_ERROR=1,
            NML_TEXT=2,
            NML_DISPLAY=3,
            OPERATOR_ERROR=11,
            OPERATOR_TEXT=12,
            OPERATOR_DISPLAY=13,
        )
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
            (11, EXPECTED_LIMIT_STOP_MESSAGE),
            (99, "informational message"),
        ]
        namespace, notifications, live_plotter = self.make_namespace(errors)

        install_axis_ui_policy(namespace)
        live_plotter.error_task()

        self.assertEqual(
            notifications.shown,
            [
                ("error", "Joint 0 following error"),
                ("error", "informational message"),
            ],
        )
        self.assertEqual(live_plotter.error_after, "scheduled-id")
        self.assertEqual(len(live_plotter.win.scheduled), 1)

    def test_all_six_linuxcnc_error_channel_types_are_explicitly_classified(self):
        namespace, notifications, live_plotter = self.make_namespace(
            [
                (1, "nml error"),
                (2, "nml text"),
                (3, "nml display"),
                (11, "operator error"),
                (12, "operator text"),
                (13, "operator display"),
            ]
        )
        output = StringIO()
        with redirect_stdout(output):
            install_axis_ui_policy(namespace)
            live_plotter.error_task()
        self.assertEqual(
            notifications.shown,
            [
                ("error", "nml error"),
                ("info", "nml text"),
                ("info", "nml display"),
                ("error", "operator error"),
                ("info", "operator text"),
                ("info", "operator display"),
            ],
        )
        for name in (
            "NML_ERROR",
            "NML_TEXT",
            "NML_DISPLAY",
            "OPERATOR_ERROR",
            "OPERATOR_TEXT",
            "OPERATOR_DISPLAY",
        ):
            self.assertIn(f"name={name}", output.getvalue())

    def test_error_channel_catalog_rejects_any_non_2_9_10_module(self):
        namespace, _notifications, _live_plotter = self.make_namespace([])
        namespace["linuxcnc"].version = "2.9.9"
        with self.assertRaisesRegex(RuntimeError, "requires LinuxCNC 2.9.10"):
            error_channel_kind_catalog(namespace["linuxcnc"])

    def test_error_channel_catalog_rejects_a_wrong_numeric_code(self):
        namespace, _notifications, _live_plotter = self.make_namespace([])
        namespace["linuxcnc"].OPERATOR_DISPLAY = 99
        with self.assertRaisesRegex(RuntimeError, "must equal 13, found 99"):
            error_channel_kind_catalog(namespace["linuxcnc"])

    def test_error_channel_catalog_rejects_a_missing_code(self):
        namespace, _notifications, _live_plotter = self.make_namespace([])
        del namespace["linuxcnc"].NML_DISPLAY
        with self.assertRaisesRegex(RuntimeError, "omitted error-channel type NML_DISPLAY"):
            error_channel_kind_catalog(namespace["linuxcnc"])

    def test_error_channel_catalog_rejects_a_non_numeric_code(self):
        namespace, _notifications, _live_plotter = self.make_namespace([])
        namespace["linuxcnc"].NML_TEXT = "not-a-number"
        with self.assertRaisesRegex(RuntimeError, "omitted error-channel type NML_TEXT"):
            error_channel_kind_catalog(namespace["linuxcnc"])

    def test_error_channel_catalog_rejects_duplicate_source_definitions(self):
        namespace, _notifications, _live_plotter = self.make_namespace([])
        namespace["linuxcnc"].NML_TEXT = 1
        definitions = (
            ("NML_ERROR", 1, "error"),
            ("NML_TEXT", 1, "info"),
        )
        with patch.object(
            notification_policy,
            "ERROR_CHANNEL_KIND_DEFINITIONS",
            definitions,
        ):
            with self.assertRaisesRegex(RuntimeError, "both equal 1"):
                error_channel_kind_catalog(namespace["linuxcnc"])

    def test_malformed_record_is_reported_and_polling_survives(self):
        namespace, notifications, live_plotter = self.make_namespace(
            [(1, "valid error"), (1,), (2, "still draining")]
        )
        output = StringIO()
        with redirect_stdout(output):
            install_axis_ui_policy(namespace)
            live_plotter.error_task()
        self.assertEqual(
            notifications.shown,
            [
                ("error", "valid error"),
                ("error", "Malformed LinuxCNC error record: (1,)"),
                ("info", "still draining"),
            ],
        )
        self.assertIn("kind=malformed", output.getvalue())
        self.assertEqual(len(live_plotter.win.scheduled), 1)

    def test_falsey_non_none_records_are_reported_not_silently_discarded(self):
        namespace, notifications, live_plotter = self.make_namespace(
            [(), False, 0, "", (2, "drain completed")]
        )
        output = StringIO()
        with redirect_stdout(output):
            install_axis_ui_policy(namespace)
            live_plotter.error_task()
        self.assertEqual(
            notifications.shown,
            [
                ("error", "Malformed LinuxCNC error record: ()"),
                ("error", "Malformed LinuxCNC error record: False"),
                ("error", "Malformed LinuxCNC error record: 0"),
                ("error", "Malformed LinuxCNC error record: ''"),
                ("info", "drain completed"),
            ],
        )
        self.assertEqual(output.getvalue().count("kind=malformed"), 4)
        self.assertEqual(len(live_plotter.win.scheduled), 1)

    def test_poll_failure_is_visible_and_the_next_poll_is_scheduled(self):
        namespace, notifications, live_plotter = self.make_namespace(
            [(2, "record before failure"), OSError("NML read failed")]
        )
        output = StringIO()
        with redirect_stdout(output):
            install_axis_ui_policy(namespace)
            live_plotter.error_task()
        self.assertEqual(
            notifications.shown,
            [
                ("info", "record before failure"),
                ("error", "LinuxCNC error-channel polling failed: NML read failed"),
            ],
        )
        self.assertIn("kind=poll_failure", output.getvalue())
        self.assertIn("OSError('NML read failed')", output.getvalue())
        self.assertEqual(live_plotter.error_after, "scheduled-id")
        self.assertEqual(len(live_plotter.win.scheduled), 1)

    def test_bad_kind_conversion_is_malformed_and_drain_continues(self):
        namespace, notifications, live_plotter = self.make_namespace(
            [("not-an-integer", "bad kind"), (3, "record after malformed kind")]
        )
        install_axis_ui_policy(namespace)
        live_plotter.error_task()
        self.assertEqual(
            notifications.shown,
            [
                (
                    "error",
                    "Malformed LinuxCNC error record: ('not-an-integer', 'bad kind')",
                ),
                ("info", "record after malformed kind"),
            ],
        )

    def test_bad_message_conversion_is_malformed_and_drain_continues(self):
        namespace, notifications, live_plotter = self.make_namespace(
            [(1, UnstringableMessage()), (2, "record after malformed message")]
        )
        install_axis_ui_policy(namespace)
        live_plotter.error_task()
        self.assertEqual(
            notifications.shown,
            [
                (
                    "error",
                    "Malformed LinuxCNC error record: (1, UnstringableMessage())",
                ),
                ("info", "record after malformed message"),
            ],
        )

    def test_installation_is_idempotent(self):
        namespace, notifications, live_plotter = self.make_namespace([(1, "one fault")])
        install_axis_ui_policy(namespace)
        installed_task = live_plotter.error_task
        installed_add = notifications.add
        install_axis_ui_policy(namespace)
        self.assertIs(live_plotter.error_task, installed_task)
        self.assertIs(notifications.add, installed_add)
        live_plotter.error_task()
        self.assertEqual(notifications.shown, [("error", "one fault")])

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
        command_file = AXIS_COMMAND_FILE
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
            "rcfile": str(AXIS_COMMAND_FILE),
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
        namespace["rcfile"] = str(AXIS_COMMAND_FILE)
        namespace["__builtins__"] = __builtins__

        command_file = AXIS_COMMAND_FILE
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
        self.assertTrue(panel_variable.get())
        self.assertEqual(panel_calls, [True])

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

    def test_unavailable_controller_keeps_status_panel_accessible_but_not_ready(self):
        namespace, component, _root, panel_variable, panel_calls = (
            self.make_pendant_namespace()
        )
        binding = install_axis_pendant_mode(namespace)
        panel_calls.clear()

        self.assertFalse(binding.set_enabled(True))
        self.assertTrue(binding.requested)
        self.assertFalse(binding.enabled)
        self.assertTrue(component[PENDANT_MODE_PIN])
        self.assertTrue(panel_variable.get())
        self.assertEqual(panel_calls, [True])

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
        self.assertFalse(binding.enabled)
        self.assertTrue(panel_variable.get())
        self.assertEqual(panel_calls, [True])

    def test_controller_unavailable_never_disarms_or_hides_visible_panel(self):
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

        self.assertTrue(component[PENDANT_MODE_PIN])
        self.assertTrue(binding.requested)
        self.assertFalse(binding.enabled)
        self.assertTrue(panel_variable.get())
        self.assertEqual(panel_calls, [True])

    def test_requested_panel_visibility_is_independent_of_every_readiness_combination(self):
        for available in (False, True):
            for ready in (False, True):
                with self.subTest(available=available, ready=ready):
                    namespace, component, root, panel_variable, panel_calls = (
                        self.make_pendant_namespace()
                    )
                    binding = install_axis_pendant_mode(namespace)
                    panel_calls.clear()
                    component[CONTROLLER_AVAILABLE_PIN] = available
                    component[CONTROLLER_READY_PIN] = ready

                    binding.set_enabled(True)
                    self.assertTrue(binding.requested)
                    self.assertTrue(component[PENDANT_MODE_PIN])
                    self.assertTrue(panel_variable.get())
                    self.assertEqual(binding.enabled, available and ready)
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

                    binding.set_enabled(False)
                    self.assertFalse(binding.requested)
                    self.assertFalse(binding.enabled)
                    self.assertFalse(component[PENDANT_MODE_PIN])
                    self.assertFalse(panel_variable.get())
                    self.assertEqual(panel_calls, [True, False])


if __name__ == "__main__":
    unittest.main()
