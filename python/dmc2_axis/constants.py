"""Stable LinuxCNC and HAL names owned by the AXIS integration."""

EXPECTED_JOG_STOP_MESSAGES = frozenset(
    (
        "Jog aborted by jog-stop",
        "Jog aborted by jog-stop-immediate",
    )
)
# hm2_modbus reports every failed transaction through LinuxCNC's operator
# error channel.  Persistent failures are separately promoted by the native
# diagnostic path to one typed MODBUS_COMMAND_DISABLED fault with retained
# command/error evidence.  Keep the raw retries in the journal without
# creating one AXIS notification per transaction.
AGGREGATED_ERROR_PREFIXES = (
    "hm2_modbus.0: error:",
)
PENDANT_MODE_PIN = "pendant-mode-enabled"
CONTROLLER_AVAILABLE_PIN = "controller-available"
CONTROLLER_READY_PIN = "controller-ready"
CONTROLLER_FAULT_PIN = "controller-fault"
CLEAR_FAULT_OPERATION_ID = "controller.clear-fault"
CLEAR_FAULT_WIDGET_PATH = ".toolbar.dmc2_clear_fault"
HOME_ALL_OPERATION_ID = "machine.home-all"
HOME_ALL_WIDGET_PATH = ".pane.top.tabs.fmanual.dmc2_homing.home_all"
HOMING_STATE_POLL_MILLISECONDS = 100
POSITION_KNOWN_PIN = "position-known"
POSITION_UNKNOWN_PIN = "position-unknown"
PENDANT_WIDGET_PATH = ".toolbar.dmc2_pendant_mode"
PENDANT_ICON_FILE = "pendant_icon.xbm"
READINESS_POLL_MILLISECONDS = 20
SPINDLE_ACTUAL_RPM_PIN = "spindle-actual-rpm"
SPINDLE_FEEDBACK_POLL_MILLISECONDS = 100
REQUIRED_LINUXCNC_VERSION = "2.9.10"
ERROR_CHANNEL_KIND_DEFINITIONS = (
    ("NML_ERROR", 1, "error"),
    ("NML_TEXT", 2, "info"),
    ("NML_DISPLAY", 3, "info"),
    ("OPERATOR_ERROR", 11, "error"),
    ("OPERATOR_TEXT", 12, "info"),
    ("OPERATOR_DISPLAY", 13, "info"),
)
