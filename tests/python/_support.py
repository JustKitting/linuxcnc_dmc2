"""Shared deterministic test paths and import boundaries."""

from pathlib import Path
import sys

PROJECT_ROOT = Path(__file__).resolve().parents[2]
PYTHON_ROOT = PROJECT_ROOT / "python"
REFERENCE_PYTHON_ROOT = PROJECT_ROOT / "reference" / "python"
FIRMWARE_PYTHON_ROOT = PROJECT_ROOT / "firmware" / "pendant_nano"
LEGACY_CONTROLLER_ROOT = PROJECT_ROOT / "reference" / "legacy_controller"
AXIS_COMMAND_FILE = PYTHON_ROOT / "dmc2_axis" / "axis_user_command.py"

for import_root in (
    PYTHON_ROOT,
    REFERENCE_PYTHON_ROOT,
    FIRMWARE_PYTHON_ROOT,
    LEGACY_CONTROLLER_ROOT,
):
    if str(import_root) not in sys.path:
        sys.path.insert(0, str(import_root))
