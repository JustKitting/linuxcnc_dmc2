#!/usr/bin/env python3
"""Report whether the profile is ready for a separate explicit live test."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
PYTHON_ROOT = PROJECT_ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from dmc2_validation import (
    DEFAULT_REQUIREMENTS,
    READY_CONFIGURATION_STATUS,
    load_requirements,
    unresolved_requirements,
)


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
