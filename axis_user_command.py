"""AXIS USER_COMMAND_FILE entry point for the DMC2 profile."""

import os as _os
import sys as _sys


_policy_directory = _os.path.dirname(_os.path.abspath(rcfile))
if _policy_directory not in _sys.path:
    _sys.path.insert(0, _policy_directory)

from axis_ui_policy import (
    install_axis_pendant_mode as _install_axis_pendant_mode,
    install_axis_ui_policy as _install_axis_ui_policy,
)


def user_hal_pins():
    """Create the AXIS-owned Pendant Mode pin before axisui becomes ready."""
    _install_axis_pendant_mode(globals())


_install_axis_ui_policy(globals())
