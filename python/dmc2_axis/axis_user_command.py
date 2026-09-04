"""AXIS USER_COMMAND_FILE entry point for the DMC2 profile."""

import importlib as _importlib
import os as _os
import sys as _sys


_package_directory = _os.path.dirname(_os.path.abspath(rcfile))
_python_root = _os.path.dirname(_package_directory)
if _python_root not in _sys.path:
    _sys.path.insert(0, _python_root)

_BOOTSTRAP_RECOVERY_TEXT = (
    "Recovery class: RELAUNCH_APPLICATION\n"
    "Recovery transition: APPLICATION_RELAUNCHED\n"
    "Clear condition: correct the named AXIS integration source and launch a "
    "matching DMC2 LinuxCNC session\n"
    "UI path: DMC2 LinuxCNC [Applications] -> Clear Fault "
    "[.toolbar.dmc2_clear_fault] -> Pendant Mode [.toolbar.dmc2_pendant_mode]"
)


def _present_bootstrap_failure(error):
    message = (
        "DMC2_AXIS_RECOVERY_BOOTSTRAP_FAILED\n"
        f"Cause: {error}\n"
        "Action: correct the named AXIS recovery-module import and relaunch "
        "DMC2 LinuxCNC from Applications\n"
        f"{_BOOTSTRAP_RECOVERY_TEXT}"
    )
    print(
        "DMC2_RECOVERY_UI_ERROR "
        "identity='DMC2_AXIS_RECOVERY_BOOTSTRAP_FAILED' "
        f"cause={error!r} recovery_class='RELAUNCH_APPLICATION' "
        "recovery_transition='APPLICATION_RELAUNCHED' "
        "ui_path='application.launch -> controller.clear-fault -> "
        "controller.pendant-mode'",
        flush=True,
    )
    try:
        notifications.add("error", message)
    except Exception as notification_error:
        print(
            "DMC2_RECOVERY_UI_PRESENTATION_FAILED "
            f"cause={notification_error!r} fallback={_BOOTSTRAP_RECOVERY_TEXT!r}",
            flush=True,
        )
        try:
            root_window.tk.call(
                "nf_dialog",
                ".dmc2_recovery_bootstrap_error",
                "DMC2 recovery bootstrap error",
                message,
                "error",
                0,
                "OK",
            )
        except Exception as dialog_error:
            print(
                "DMC2_RECOVERY_UI_FALLBACK_DIALOG_FAILED "
                f"cause={dialog_error!r} bootstrap_error={error!r} "
                f"notification_error={notification_error!r} "
                f"fallback={message!r}",
                flush=True,
            )


try:
    from dmc2_axis.recovery_ui import (
        present_recovery_ui_error as _present_recovery_ui_error,
    )
    from dmc2_axis.ui_fault import AxisUiFault as _AxisUiFault
    from dmc2_axis.ui_fault import AxisUiFaultKind as _AxisUiFaultKind
except Exception as _recovery_bootstrap_error:
    _recovery_bootstrap_ready = False
    _present_bootstrap_failure(_recovery_bootstrap_error)
else:
    _recovery_bootstrap_ready = True


def _install_extension(module_name, installer_name, fault_kind):
    if not _recovery_bootstrap_ready:
        return None
    try:
        module = _importlib.import_module(module_name, package="dmc2_axis")
        installer = getattr(module, installer_name)
        return installer(globals())
    except Exception as error:
        try:
            _present_recovery_ui_error(
                globals(),
                fault=_AxisUiFault(fault_kind, error),
            )
        except Exception as presentation_error:
            _present_bootstrap_failure(
                f"{module_name}.{installer_name}: {error}; "
                f"typed recovery presentation also failed: {presentation_error}"
            )
        return None


def user_hal_pins():
    """Create DMC2 AXIS pins and widgets before axisui becomes ready."""
    if not _recovery_bootstrap_ready:
        return
    _install_extension(
        ".base_controls",
        "install_axis_base_controls",
        _AxisUiFaultKind.BASE_RECOVERY_CONTROLS_INSTALL_FAILED,
    )
    _install_extension(
        ".pendant_mode",
        "install_axis_pendant_mode",
        _AxisUiFaultKind.PENDANT_MODE_CONTROL_INSTALL_FAILED,
    )
    _install_extension(
        ".spindle_feedback",
        "install_axis_spindle_feedback",
        _AxisUiFaultKind.SPINDLE_FEEDBACK_UI_INSTALL_FAILED,
    )


if _recovery_bootstrap_ready:
    _install_extension(
        ".notifications",
        "install_axis_ui_policy",
        _AxisUiFaultKind.AXIS_NOTIFICATION_POLICY_INSTALL_FAILED,
    )
    _install_extension(
        ".run_guard",
        "install_axis_run_guard",
        _AxisUiFaultKind.AXIS_RUN_GUARD_INSTALL_FAILED,
    )
