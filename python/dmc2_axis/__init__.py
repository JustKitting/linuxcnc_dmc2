"""DMC2 extensions for LinuxCNC's AXIS user interface."""

from .constants import (
    CLEAR_FAULT_OPERATION_ID,
    CLEAR_FAULT_WIDGET_PATH,
    CONTROLLER_AVAILABLE_PIN,
    CONTROLLER_FAULT_PIN,
    CONTROLLER_READY_PIN,
    ERROR_CHANNEL_KIND_DEFINITIONS,
    EXPECTED_JOG_STOP_MESSAGES,
    HOME_ALL_OPERATION_ID,
    HOME_ALL_WIDGET_PATH,
    HOMING_STATE_POLL_MILLISECONDS,
    PENDANT_ICON_FILE,
    PENDANT_MODE_PIN,
    PENDANT_WIDGET_PATH,
    POSITION_KNOWN_PIN,
    POSITION_UNKNOWN_PIN,
    READINESS_POLL_MILLISECONDS,
    REQUIRED_LINUXCNC_VERSION,
    SPINDLE_ACTUAL_RPM_PIN,
    SPINDLE_FEEDBACK_POLL_MILLISECONDS,
)
from .base_controls import HomingSectionBinding, install_axis_base_controls
from .notifications import (
    error_channel_kind_catalog,
    install_axis_ui_policy,
    should_suppress_notification,
)
from .diagnostic_journal import (
    DiagnosticEvent,
    DiagnosticJournalReader,
    default_diagnostic_journal_path,
)
from .pendant_mode import PendantModeBinding, install_axis_pendant_mode
from .run_guard import AxisRunGuard, install_axis_run_guard
from .spindle_feedback import SpindleFeedbackBinding, install_axis_spindle_feedback

__all__ = [
    "CLEAR_FAULT_OPERATION_ID",
    "CLEAR_FAULT_WIDGET_PATH",
    "CONTROLLER_AVAILABLE_PIN",
    "CONTROLLER_FAULT_PIN",
    "CONTROLLER_READY_PIN",
    "DiagnosticEvent",
    "DiagnosticJournalReader",
    "ERROR_CHANNEL_KIND_DEFINITIONS",
    "EXPECTED_JOG_STOP_MESSAGES",
    "HOME_ALL_OPERATION_ID",
    "HOME_ALL_WIDGET_PATH",
    "HOMING_STATE_POLL_MILLISECONDS",
    "HomingSectionBinding",
    "PENDANT_ICON_FILE",
    "PENDANT_MODE_PIN",
    "PENDANT_WIDGET_PATH",
    "POSITION_KNOWN_PIN",
    "POSITION_UNKNOWN_PIN",
    "PendantModeBinding",
    "READINESS_POLL_MILLISECONDS",
    "REQUIRED_LINUXCNC_VERSION",
    "SPINDLE_ACTUAL_RPM_PIN",
    "SPINDLE_FEEDBACK_POLL_MILLISECONDS",
    "SpindleFeedbackBinding",
    "AxisRunGuard",
    "error_channel_kind_catalog",
    "default_diagnostic_journal_path",
    "install_axis_pendant_mode",
    "install_axis_run_guard",
    "install_axis_base_controls",
    "install_axis_spindle_feedback",
    "install_axis_ui_policy",
    "should_suppress_notification",
]
