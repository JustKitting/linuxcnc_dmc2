"""Canonical project paths used by offline validation."""

from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[2]
ROOT = PROJECT_ROOT
LIVE_DIR = PROJECT_ROOT / "live"
SIM_DIR = PROJECT_ROOT / "sim"
