"""Composition root for the complete offline profile validation."""

import shutil

from .common import (
    executable_hal_text,
    pin_names_from_panel,
    pin_names_from_postgui,
    read_ini,
    source_tree_text,
)
from .paths import LIVE_DIR, PROJECT_ROOT as ROOT, SIM_DIR
from .probing import (
    validate_first_tool_height_test,
    validate_homing_style_tool_height_test,
    validate_probe_test_programs,
)
from .profile import validate_live_hal, validate_live_ini
from .readiness import (
    READY_CONFIGURATION_STATUS,
    load_requirements,
    unresolved_requirements,
)
from .spindle import validate_h100_spindle_integration, validate_spindle_test_operation
from .task_monitor import validate_task_monitor_contract


def validate() -> list[str]:
    checks: list[str] = []

    panel_pins = pin_names_from_panel(ROOT / "live" / "ui" / "status_panel.xml")
    postgui_pins = pin_names_from_postgui(ROOT / "live" / "hal" / "status_postgui.hal")
    if panel_pins != postgui_pins:
        missing_nets = sorted(panel_pins - postgui_pins)
        missing_widgets = sorted(postgui_pins - panel_pins)
        raise AssertionError(
            f"panel/postgui mismatch: missing nets={missing_nets}, "
            f"missing widgets={missing_widgets}"
        )
    checks.append(f"PyVCP XML and {len(panel_pins)} post-GUI pins agree")

    axis_policy = source_tree_text(ROOT / "python" / "dmc2_axis", ".py")
    if 'EXPECTED_LIMIT_STOP_MESSAGE = "Jog aborted by jog-stop-immediate"' not in axis_policy:
        raise AssertionError("AXIS policy does not identify the exact expected limit stop")
    if "notifications.add = add_without_covering_status_panel" not in axis_policy:
        raise AssertionError("AXIS notifications are not moved away from the status panel")
    required_pendant_mode_policy = (
        'PENDANT_MODE_PIN = "pendant-mode-enabled"',
        'CONTROLLER_AVAILABLE_PIN = "controller-available"',
        'CONTROLLER_READY_PIN = "controller-ready"',
        'PENDANT_WIDGET_PATH = ".toolbar.dmc2_pendant_mode"',
        '"-after",\n        ".toolbar.clear_plot"',
        "component[PENDANT_MODE_PIN] = False",
        "component[PENDANT_MODE_PIN] = True",
        "self.enabled = available and ready",
        "self._set_panel_visible(True)",
        '"-state",\n            "normal"',
    )
    missing = [
        token for token in required_pendant_mode_policy if token not in axis_policy
    ]
    if missing:
        raise AssertionError(f"AXIS Pendant Mode policy is incomplete: {missing}")
    icon = (
        ROOT / "python" / "dmc2_axis" / "pendant_icon.xbm"
    ).read_text(encoding="ascii")
    if (
        "#define dmc2_pendant_width 24" not in icon
        or "#define dmc2_pendant_height 24" not in icon
    ):
        raise AssertionError("AXIS pendant toolbar icon is not the expected 24x24 XBM")
    postgui = (ROOT / "live" / "hal" / "status_postgui.hal").read_text(
        encoding="utf-8"
    )
    if (
        "axisui.pendant-mode-enabled => "
        "dmc2-pendant-control.pendant-mode-enabled"
    ) not in postgui:
        raise AssertionError("AXIS Pendant Mode is not connected to the supervisor")
    required_postgui_handshake = (
        "dmc2-controller-available     => axisui.controller-available",
        "dmc2-controller-ready         => axisui.controller-ready pyvcp.controller-ready",
        "dmc2-controller-estop-reset-request => halui.estop.reset",
        "dmc2-controller-machine-on-request  => halui.machine.on",
    )
    missing = [
        token for token in required_postgui_handshake if token not in postgui
    ]
    if missing:
        raise AssertionError(
            f"Pendant Mode readiness/state-request handshake is incomplete: {missing}"
        )
    live_pendant = (LIVE_DIR / "pendant.hal").read_text(encoding="utf-8")
    if "python3" in executable_hal_text(LIVE_DIR / "pendant.hal"):
        raise AssertionError("live pendant control still invokes Python")
    checks.append(
        "AXIS hides only the expected realtime limit-stop popup and preserves other notifications"
    )
    checks.append(
        "AXIS keeps the requested Pendant panel visible through readiness and fault transitions while realtime control remains gated"
    )
    checks.append(
        "LinuxCNC E-stop-reset, machine-on, and finite jog requests are delegated to native HALUI/motion pins"
    )
    checks.append(
        "compiled Rust owns servo-thread policy, exact count verification, and the real milltask heartbeat"
    )
    checks.append(validate_task_monitor_contract())

    sim_config = read_ini(SIM_DIR / "monitor.ini")
    required_sim_sections = {
        "EMC", "DISPLAY", "TASK", "RS274NGC", "EMCMOT", "EMCIO", "HAL",
        "TRAJ", "KINS", "AXIS_X", "AXIS_Y", "AXIS_Z", "JOINT_0", "JOINT_1",
        "JOINT_2",
    }
    missing_sections = sorted(required_sim_sections - set(sim_config.sections()))
    if missing_sections:
        raise AssertionError(f"simulation INI missing sections: {missing_sections}")
    if sim_config["EMC"]["MACHINE"] != "DMC2-OFFLINE-MONITOR-SIMULATION":
        raise AssertionError("simulation INI is not unmistakably labelled")
    checks.append("offline LinuxCNC INI has all required sections")

    sim_text = executable_hal_text(
        SIM_DIR / "core_sim.hal",
        SIM_DIR / "status_sources.hal",
        ROOT / "reference" / "hal" / "pendant_replay.hal",
    ) + "\n" + (SIM_DIR / "monitor.ini").read_text(encoding="utf-8")
    forbidden_sim_tokens = ("hm2_eth", "hm2_7i95", "/dev/tty", "motion.probe-input")
    present = [token for token in forbidden_sim_tokens if token in sim_text]
    if present:
        raise AssertionError(f"offline simulator contains hardware token(s): {present}")
    checks.append("offline simulator contains no Mesa, serial-device, or probe-motion route")

    hal_files = list(ROOT.glob("*.hal")) + list(SIM_DIR.glob("*.hal")) + list(
        LIVE_DIR.glob("*.hal")
    )
    continued = [
        str(path.relative_to(ROOT))
        for path in hal_files
        if any(line.rstrip().endswith("\\") for line in path.read_text().splitlines())
    ]
    if continued:
        raise AssertionError(
            f"HAL files contain unsupported shell-style continuations: {continued}"
        )
    checks.append("HAL commands contain no shell-style line continuations")

    checks.append(validate_live_ini())
    checks.extend(validate_live_hal())
    checks.append(validate_h100_spindle_integration())
    checks.append(validate_spindle_test_operation())
    checks.append(validate_probe_test_programs())
    checks.append(validate_first_tool_height_test())
    checks.append(validate_homing_style_tool_height_test())

    readiness = load_requirements(ROOT / "live_requirements.json")
    if readiness.get("configuration_status") != READY_CONFIGURATION_STATUS:
        raise AssertionError("live profile is not ready for explicit hardware validation")
    unresolved = unresolved_requirements(readiness)
    if unresolved:
        raise AssertionError(
            "blocking requirements remain: "
            + ", ".join(item["id"] for item in unresolved)
        )
    deferred = [item for item in readiness["requirements"] if not item["blocking"]]
    if not deferred or any(item["status"] == "confirmed" for item in deferred):
        raise AssertionError("deferred integrations are not explicitly preserved")
    checks.append("accepted profile has no blocker; unapproved integrations remain deferred")

    launcher_root = ROOT / "rust" / "crates" / "dmc2-launcher"
    launcher = source_tree_text(launcher_root / "src", ".rs")
    if (ROOT / "scripts" / "launch_live.py").exists() or (
        ROOT / "python" / "dmc2_runtime"
    ).exists():
        raise AssertionError("Python live-launch wrapper still exists")
    required_launcher_tokens = (
        'pub enum Mode {',
        '"--live" if !live => live = true',
        'Mode::Validate => Action::Validate',
        'Mode::Direct =>',
        'Mode::Persistent =>',
        'replace_process(&self, command:',
        'layout.project.join("native/bin/dmc2-linuxcnc")',
        'validate_embedded_inputs(platform, layout)?',
        'validate_deployments(platform, layout)?',
    )
    missing = [token for token in required_launcher_tokens if token not in launcher]
    if missing:
        raise AssertionError(f"compiled live-launch boundary is incomplete: {missing}")
    if shutil.which("linuxcnc") is None:
        raise AssertionError("LinuxCNC executable is unavailable")
    version_tokens = (
        'EXPECTED_LINUXCNC_VERSION: &[u8] = b"2.9.10\\n"',
        'OsString::from("LINUXCNCVERSION")',
        'output.stdout != EXPECTED_LINUXCNC_VERSION',
    )
    if any(token not in launcher for token in version_tokens):
        raise AssertionError("live launcher is not locked to LinuxCNC 2.9.10")
    checks.append(
        "compiled launcher defaults to validation, requires explicit --live, and locks LinuxCNC 2.9.10"
    )

    realtime_tokens = (
        'pub const REALTIME_ENVIRONMENT_NAME: &str = "LINUXCNC_FORCE_REALTIME"',
        '"--setenv=LINUXCNC_FORCE_REALTIME=1"',
        'OsString::from(REALTIME_ENVIRONMENT_VALUE)',
    )
    if any(token not in launcher for token in realtime_tokens):
        raise AssertionError(
            "live launcher does not force realtime scheduling in both launch paths"
        )
    checks.append("persistent and direct launch paths force realtime scheduling")

    return checks
