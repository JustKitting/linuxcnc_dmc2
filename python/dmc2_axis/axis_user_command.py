"""AXIS USER_COMMAND_FILE entry point for the DMC2 profile."""

import importlib as _importlib
import os as _os
import sys as _sys
from functools import partial as _partial


_package_directory = _os.path.dirname(_os.path.abspath(rcfile))
_python_root = _os.path.dirname(_package_directory)
if _python_root not in _sys.path:
    _sys.path.insert(0, _python_root)

_BOOTSTRAP_RECOVERY_TEXT = (
    "Recovery class: RELAUNCH_APPLICATION\n"
    "Recovery transition: APPLICATION_RELAUNCHED\n"
    "Clear condition: correct the named AXIS integration source and launch a "
    "matching DMC2 LinuxCNC session\n"
    "Recovery controls: DMC2 LinuxCNC -> Clear Fault -> Pendant Mode"
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


# Install the closed execution boundary before importing optional extensions.
# No installation rollback may restore a stock Run/Step path. Recovery controls
# are deliberately absent from this catalog and install independently below.
_dmc2_stock_execution = {}
_dmc2_execution_interlock_errors = []
_DMC2_EXECUTION_CONTROLS = (("task_run", "r"), ("task_step", "t"))


def _dispatch_program_execution(command_name, *args):
    guard = getattr(live_plotter, "_dmc2_axis_run_guard", None)
    if guard is None or _dmc2_execution_interlock_errors:
        _present_bootstrap_failure(
            "Run and Step are blocked because the script loader/execution guard "
            "is unavailable. Abort, Clear Fault, and Pendant Mode remain "
            "independent. Correct the named installation error and relaunch "
            f"through Applications. Details: {_dmc2_execution_interlock_errors!r}"
        )
        return "break"
    return guard.submit(command_name, *args)


for _command_name, _key in _DMC2_EXECUTION_CONTROLS:
    _dmc2_stock_execution[_command_name] = getattr(commands, _command_name)
    _callback = _partial(_dispatch_program_execution, _command_name)
    setattr(commands, _command_name, _callback)
    # Remove both old routes before registering their guarded replacements.
    # A registration failure leaves a missing/blocked execution command, not
    # an alias to the original unguarded Tcl or keyboard callback.
    for _route, _install in (
        ("keyboard removal", _partial(root_window.tk.call, "bind", root_window._w, _key, "")),
        ("Tcl removal", _partial(root_window.tk.call, "rename", _command_name, "")),
        ("Tcl guard", _partial(root_window.tk.createcommand, _command_name, _callback)),
        ("keyboard guard", _partial(root_window.bind, _key, _callback)),
    ):
        try:
            _install()
        except Exception as _error:
            _dmc2_execution_interlock_errors.append(f"{_command_name} {_route}: {_error}")

if _dmc2_execution_interlock_errors:
    _present_bootstrap_failure("; ".join(_dmc2_execution_interlock_errors))


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
        ".script_loader",
        "install_axis_script_loader",
        _AxisUiFaultKind.SCRIPT_LOADER_INSTALL_FAILED,
    )
    _install_extension(
        ".run_guard",
        "install_axis_run_guard",
        _AxisUiFaultKind.AXIS_RUN_GUARD_INSTALL_FAILED,
    )
