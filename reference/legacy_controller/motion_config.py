from __future__ import annotations

from fractions import Fraction
from pathlib import Path


CONFIG_PATH = Path(__file__).resolve().parents[2] / "config" / "machine-pulses.conf"
REQUIRED_KEYS = {"MOTOR_PULSES_PER_REV", "REFERENCE_PULSES_PER_REV"}


def _load_positive_integer_settings(path: Path) -> dict[str, int]:
    settings: dict[str, int] = {}
    for line_number, raw_line in enumerate(path.read_text().splitlines(), start=1):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if "=" not in line:
            raise RuntimeError(f"{path}:{line_number}: expected NAME=INTEGER")
        key, raw_value = (part.strip() for part in line.split("=", 1))
        if key not in REQUIRED_KEYS:
            raise RuntimeError(f"{path}:{line_number}: unknown setting {key!r}")
        if key in settings:
            raise RuntimeError(f"{path}:{line_number}: duplicate setting {key!r}")
        if not raw_value.isdecimal() or int(raw_value) <= 0:
            raise RuntimeError(f"{path}:{line_number}: {key} must be positive integer")
        settings[key] = int(raw_value)

    missing = REQUIRED_KEYS - settings.keys()
    if missing:
        raise RuntimeError(f"{path}: missing settings: {', '.join(sorted(missing))}")
    return settings


_SETTINGS = _load_positive_integer_settings(CONFIG_PATH)
MOTOR_PULSES_PER_REV = _SETTINGS["MOTOR_PULSES_PER_REV"]
REFERENCE_PULSES_PER_REV = _SETTINGS["REFERENCE_PULSES_PER_REV"]
PULSE_SCALE = Fraction(MOTOR_PULSES_PER_REV, REFERENCE_PULSES_PER_REV)


def scale_reference_value(reference_value: int) -> int:
    if isinstance(reference_value, bool) or not isinstance(reference_value, int):
        raise TypeError("reference pulse value must be an integer")
    if reference_value < 0:
        raise ValueError("reference pulse value must be non-negative")
    scaled = reference_value * PULSE_SCALE
    if scaled.denominator != 1:
        raise RuntimeError(
            f"{reference_value} reference pulses cannot be scaled exactly from "
            f"{REFERENCE_PULSES_PER_REV} to {MOTOR_PULSES_PER_REV} pulses/revolution"
        )
    return scaled.numerator
