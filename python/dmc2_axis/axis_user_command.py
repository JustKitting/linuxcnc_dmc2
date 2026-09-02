"""AXIS USER_COMMAND_FILE entry point for the DMC2 profile."""

import os as _os
import sys as _sys


_package_directory = _os.path.dirname(_os.path.abspath(rcfile))
_python_root = _os.path.dirname(_package_directory)
if _python_root not in _sys.path:
    _sys.path.insert(0, _python_root)

from dmc2_axis import (
    install_axis_base_controls as _install_axis_base_controls,
    install_axis_pendant_mode as _install_axis_pendant_mode,
    install_axis_run_guard as _install_axis_run_guard,
    install_axis_spindle_feedback as _install_axis_spindle_feedback,
    install_axis_ui_policy as _install_axis_ui_policy,
)


def user_hal_pins():
    """Create DMC2 AXIS pins and widgets before axisui becomes ready."""
    _install_axis_spindle_feedback(globals())
    _install_axis_pendant_mode(globals())
    _install_axis_base_controls(globals())


_install_axis_ui_policy(globals())
_install_axis_run_guard(globals())
