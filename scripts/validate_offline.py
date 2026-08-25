#!/usr/bin/env python3
"""Validate the accepted DMC2 profile without opening serial, Mesa, or NML."""

from __future__ import annotations

import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
PYTHON_ROOT = PROJECT_ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from dmc2_validation import validate


def main() -> int:
    try:
        checks = validate()
    except Exception as error:
        print(f"OFFLINE VALIDATION FAILED: {error}", file=sys.stderr)
        return 1
    for check in checks:
        print(f"PASS: {check}")
    print("PASS: validation opened neither NML, a serial device, nor Mesa hardware")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
