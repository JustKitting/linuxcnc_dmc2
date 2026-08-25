"""DMC2 extensions for LinuxCNC's AXIS user interface."""

from .constants import (
    CONTROLLER_AVAILABLE_PIN,
    CONTROLLER_READY_PIN,
    ERROR_CHANNEL_KIND_DEFINITIONS,
    EXPECTED_LIMIT_STOP_MESSAGE,
    PENDANT_ICON_FILE,
    PENDANT_MODE_PIN,
    PENDANT_WIDGET_PATH,
    READINESS_POLL_MILLISECONDS,
    REQUIRED_LINUXCNC_VERSION,
)
from .notifications import (
    error_channel_kind_catalog,
    install_axis_ui_policy,
    should_suppress_notification,
)
from .pendant_mode import PendantModeBinding, install_axis_pendant_mode

__all__ = [
    "CONTROLLER_AVAILABLE_PIN",
    "CONTROLLER_READY_PIN",
    "ERROR_CHANNEL_KIND_DEFINITIONS",
    "EXPECTED_LIMIT_STOP_MESSAGE",
    "PENDANT_ICON_FILE",
    "PENDANT_MODE_PIN",
    "PENDANT_WIDGET_PATH",
    "PendantModeBinding",
    "READINESS_POLL_MILLISECONDS",
    "REQUIRED_LINUXCNC_VERSION",
    "error_channel_kind_catalog",
    "install_axis_pendant_mode",
    "install_axis_ui_policy",
    "should_suppress_notification",
]
