#!/usr/bin/env python3
"""Report whether the profile is ready for a separate explicit live test."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

DEFAULT_REQUIREMENTS = Path(__file__).with_name("live_requirements.json")
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", nargs="?", type=Path, default=DEFAULT_REQUIREMENTS)
    args = parser.parse_args()
    data = load_requirements(args.path)
    unresolved = unresolved_requirements(data)
    if data.get("configuration_status") != READY_CONFIGURATION_STATUS or unresolved:
        print("LIVE CONFIGURATION LOCKED")
        for item in unresolved:
            print(f"- {item['id']}: {item['status']} — {item['reason']}")
        return 2
    print("READY FOR EXPLICIT HARDWARE VALIDATION")
    print("This checker opened neither LinuxCNC, the Nano serial port, nor Mesa hardware.")
    deferred = [item for item in data["requirements"] if not item.get("blocking", True)]
    for item in deferred:
        print(f"- deferred: {item['id']} — {item['status']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
