#!/usr/bin/env python3
"""Validate or explicitly launch the single-owner DMC2 LinuxCNC profile."""

from __future__ import annotations

import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
PYTHON_ROOT = PROJECT_ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from dmc2_runtime.launcher import main


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as error:
        print(f"LIVE LAUNCH REFUSED: {error}")
        raise SystemExit(2)
