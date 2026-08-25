"""Accepted-profile readiness document parsing."""

from __future__ import annotations

import json
from pathlib import Path

from .paths import PROJECT_ROOT

DEFAULT_REQUIREMENTS = PROJECT_ROOT / "live_requirements.json"
READY_CONFIGURATION_STATUS = "READY_FOR_EXPLICIT_HARDWARE_VALIDATION"
SATISFIED_BLOCKING_STATUSES = {
    "confirmed",
    "accepted_provisional",
    "implemented_offline",
}


def load_requirements(path: Path) -> dict:
    with path.open(encoding="utf-8") as stream:
        data = json.load(stream)
    if data.get("schema_version") != 2:
        raise ValueError("unsupported live requirements schema")
    if not isinstance(data.get("requirements"), list):
        raise ValueError("requirements must be a list")
    return data


def unresolved_requirements(data: dict) -> list[dict]:
    return [
        item
        for item in data["requirements"]
        if item.get("blocking", True)
        and (
            item.get("status") not in SATISFIED_BLOCKING_STATUSES
            or item.get("value") is None
        )
    ]
