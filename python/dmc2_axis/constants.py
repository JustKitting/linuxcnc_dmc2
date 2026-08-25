"""Stable LinuxCNC and HAL names owned by the AXIS integration."""

EXPECTED_LIMIT_STOP_MESSAGE = "Jog aborted by jog-stop-immediate"
PENDANT_MODE_PIN = "pendant-mode-enabled"
CONTROLLER_AVAILABLE_PIN = "controller-available"
CONTROLLER_READY_PIN = "controller-ready"
PENDANT_WIDGET_PATH = ".toolbar.dmc2_pendant_mode"
PENDANT_ICON_FILE = "pendant_icon.xbm"
READINESS_POLL_MILLISECONDS = 20
REQUIRED_LINUXCNC_VERSION = "2.9.10"
ERROR_CHANNEL_KIND_DEFINITIONS = (
    ("NML_ERROR", 1, "error"),
    ("NML_TEXT", 2, "info"),
    ("NML_DISPLAY", 3, "info"),
    ("OPERATOR_ERROR", 11, "error"),
    ("OPERATOR_TEXT", 12, "info"),
    ("OPERATOR_DISPLAY", 13, "info"),
)
