#!/usr/bin/env python3
"""Validate or explicitly launch the single-owner DMC2 LinuxCNC profile."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

from check_live_readiness import (
    READY_CONFIGURATION_STATUS,
    load_requirements,
    unresolved_requirements,
)
from validate_offline import validate as validate_offline_profile


ROOT = Path(__file__).resolve().parent
INI = ROOT / "live" / "dmc2.ini"
REQUIREMENTS = ROOT / "live_requirements.json"
PERSISTENT_UNIT = "dmc2-linuxcnc"
REALTIME_ENVIRONMENT = "LINUXCNC_FORCE_REALTIME=1"
EXPECTED_LINUXCNC_VERSION = "2.9.10"
REALTIME_MODULE = Path("/usr/lib/linuxcnc/modules/dmc2_rt.so")
STAGED_REALTIME_MODULE = ROOT / "rust" / "target" / "release" / "libdmc2_rt.so"


def running_process(pattern: str) -> bool:
    completed = subprocess.run(
        ["pgrep", "-f", pattern],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return completed.returncode == 0


def validate_launch_files() -> list[str]:
    checks: list[str] = []
    data = load_requirements(REQUIREMENTS)
    unresolved = unresolved_requirements(data)
    if data.get("configuration_status") != READY_CONFIGURATION_STATUS:
        raise RuntimeError(
            f"configuration status is {data.get('configuration_status')!r}, "
            f"not {READY_CONFIGURATION_STATUS!r}"
        )
    if unresolved:
        raise RuntimeError(
            "blocking requirements remain: "
            + ", ".join(item["id"] for item in unresolved)
        )
    checks.append("accepted provisional profile has no blocking requirement")

    required_files = (
        INI,
        ROOT / "live" / "machine.hal",
        ROOT / "live" / "pendant.hal",
        ROOT / "native" / "bin" / "dmc2-serial-bridge",
        ROOT / "native" / "bin" / "dmc2-task-monitor",
        STAGED_REALTIME_MODULE,
        REALTIME_MODULE,
        ROOT / "status_panel.xml",
        ROOT / "axis_user_command.py",
        ROOT / "axis_ui_policy.py",
    )
    missing = [str(path) for path in required_files if not path.is_file()]
    if missing:
        raise RuntimeError("missing live file(s): " + ", ".join(missing))
    if REALTIME_MODULE.read_bytes() != STAGED_REALTIME_MODULE.read_bytes():
        raise RuntimeError(
            "installed dmc2_rt.so does not match the offline-tested staged module"
        )
    checks.append("all live configuration files exist")

    if shutil.which("linuxcnc") is None or shutil.which("linuxcnc_var") is None:
        raise RuntimeError("LinuxCNC executables are unavailable")
    version_result = subprocess.run(
        ["linuxcnc_var", "LINUXCNCVERSION"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    installed_version = version_result.stdout.strip()
    if version_result.returncode != 0 or installed_version != EXPECTED_LINUXCNC_VERSION:
        raise RuntimeError(
            "refusing LinuxCNC version "
            f"{installed_version or 'unknown'}; expected {EXPECTED_LINUXCNC_VERSION}"
        )
    checks.append(f"LinuxCNC {EXPECTED_LINUXCNC_VERSION} is installed")

    try:
        profile_checks = validate_offline_profile()
    except Exception as error:
        raise RuntimeError(f"exact offline profile validation failed: {error}") from error
    checks.append(f"exact offline profile validation passed ({len(profile_checks)} checks)")
    return checks


def assert_no_owner_conflict() -> None:
    conflict_patterns = (
        "[l]inuxcnc.*dmc2.ini",
        "[p]endant_cnc/control.py.*--live",
        "[h]alrun",
        "[r]tapi_app",
    )
    conflicts = [pattern for pattern in conflict_patterns if running_process(pattern)]
    if conflicts:
        raise RuntimeError(
            "another LinuxCNC/HAL/Mesa owner may be active: " + ", ".join(conflicts)
        )


def start_persistent_service() -> int:
    if shutil.which("systemd-run") is None:
        raise RuntimeError("systemd-run is unavailable; persistent launch refused")
    command = [
        "systemd-run",
        "--user",
        f"--unit={PERSISTENT_UNIT}",
        f"--setenv={REALTIME_ENVIRONMENT}",
        "--collect",
        "--property=KillMode=control-group",
        "--property=Restart=no",
        f"--working-directory={INI.parent}",
        sys.executable,
        str(ROOT / "launch_live.py"),
        "--live",
    ]
    completed = subprocess.run(
        command,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        check=False,
    )
    output = completed.stdout.strip()
    if completed.returncode != 0:
        raise RuntimeError(
            "persistent LinuxCNC launch failed"
            + (f": {output}" if output else "")
        )
    if output:
        print(output)
    print(
        f"PERSISTENT LIVE UNIT STARTED: {PERSISTENT_UNIT}.service; "
        f"inspect with 'journalctl --user-unit {PERSISTENT_UNIT} -f'"
    )
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--live",
        action="store_true",
        help="explicitly exec LinuxCNC with the hardware configuration",
    )
    parser.add_argument(
        "--persistent",
        action="store_true",
        help=(
            "with --live, launch LinuxCNC as a transient user service so it "
            "survives the initiating terminal"
        ),
    )
    args = parser.parse_args(argv)

    if args.persistent and not args.live:
        parser.error("--persistent requires --live")

    checks = validate_launch_files()
    for check in checks:
        print(f"PASS: {check}")
    if not args.live:
        print("VALIDATION ONLY — LinuxCNC, Nano serial, and Mesa were not opened.")
        return 0

    assert_no_owner_conflict()
    if args.persistent:
        return start_persistent_service()
    # Raspberry Pi's current PREEMPT_RT kernel does not expose the legacy
    # /sys/kernel/realtime flag that LinuxCNC 2.9.10 probes.  The system profile
    # already requests this override; set it explicitly here so direct and
    # systemd-owned launches select the same POSIX SCHED_FIFO implementation.
    os.environ["LINUXCNC_FORCE_REALTIME"] = "1"
    os.chdir(INI.parent)
    os.execvp("linuxcnc", ["linuxcnc", "-r", str(INI)])
    return 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as error:
        print(f"LIVE LAUNCH REFUSED: {error}")
        raise SystemExit(2)
