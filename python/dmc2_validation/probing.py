"""No-motion and moving probe-operation validation."""

import re

from .paths import LIVE_DIR, PROJECT_ROOT as ROOT
from .spindle import executable_gcode_text


def validate_probe_test_programs() -> str:
    test_path = LIVE_DIR / "nc_files" / "puck-contact-no-motion-test.ngc"
    abort_path = LIVE_DIR / "nc_files" / "dmc2_abort.ngc"
    test = executable_gcode_text(test_path)
    abort = executable_gcode_text(abort_path)

    required_test_tokens = (
        "#<contact_timeout_seconds> = 300.0",
        "M64 P0",
        "M66 P1 L3 Q1.0",
        "M66 P0 L1 Q#<contact_timeout_seconds>",
        "M66 P1 L4 Q1.0",
        "M66 P0 L4 Q1.0",
        "#<contact_result> = #5399",
        "M65 P0",
    )
    missing = [token for token in required_test_tokens if token not in test]
    if missing:
        raise AssertionError(f"no-motion puck test is incomplete: {missing}")

    forbidden_motion = re.findall(
        r"\b(?:G(?:0?0|0?1|0?2|0?3|38(?:\.\d+)?)|M[345])\b",
        test,
        flags=re.IGNORECASE,
    )
    if forbidden_motion:
        raise AssertionError(
            "no-motion puck test contains motion/spindle words: "
            f"{forbidden_motion}"
        )
    if "M64 P0" in abort or "M65 P0" not in abort:
        raise AssertionError("abort handler does not unconditionally clear probe power")
    abort_forbidden = re.findall(
        r"\b(?:G(?:0?0|0?1|0?2|0?3|38(?:\.\d+)?)|M[345])\b",
        abort,
        flags=re.IGNORECASE,
    )
    if abort_forbidden:
        raise AssertionError(
            f"probe abort handler contains motion/spindle words: {abort_forbidden}"
        )
    return (
        "LinuxCNC G-code integration test verifies its power gate, has a 300-second "
        "fail-off window and abort cleanup, and has no axis or spindle-start "
        "command"
    )


def validate_first_tool_height_test() -> str:
    path = LIVE_DIR / "nc_files" / "tool-height-first-test.ngc"
    program = executable_gcode_text(path)

    required = (
        "#<puck_machine_x> = 288.125",
        "#<puck_machine_y> = 152.955",
        "#<puck_height_mm> = 19.40",
        "#<z_home_machine> = 135.0",
        "#<probe_target_z> = 0.0",
        "#<probe_feed_mm_min> = 6.0",
        "M5",
        "M0",
        "M64 P0",
        "M66 P1 L3 Q1.0",
        "G38.2 Z#<probe_target_z> F#<probe_feed_mm_min>",
        "#<probe_machine_z> = #5063",
        "#<plate_contact_machine_z> = [#<probe_machine_z> - #<puck_height_mm>]",
        "M65 P0",
        "M66 P1 L4 Q1.0",
        "G53 G0 Z#<z_home_machine>",
    )
    missing = [token for token in required if token not in program]
    if missing:
        raise AssertionError(f"first moving tool-height test is incomplete: {missing}")

    gate = program.index("M0")
    power_on = program.index("M64 P0")
    probe = program.index("G38.2 Z#<probe_target_z> F#<probe_feed_mm_min>")
    power_off = program.index("M65 P0", probe)
    return_home = program.index("G53 G0 Z#<z_home_machine>")
    if not gate < power_on < probe < power_off < return_home:
        raise AssertionError(
            "tool-height test order must be gate, power on, probe, power off, return"
        )

    if len(re.findall(r"\bG38\.2\b", program, flags=re.IGNORECASE)) != 1:
        raise AssertionError("first tool-height test must contain exactly one probe move")
    if re.search(r"\bG(?:0?0|0?1|38\.2)\b[^\n]*\b[XY]", program, flags=re.IGNORECASE):
        raise AssertionError("first tool-height test must not command X or Y motion")
    if re.search(r"\bM[34]\b", program, flags=re.IGNORECASE):
        raise AssertionError("first tool-height test contains a spindle-start command")

    return (
        "first moving tool-height test holds at an operator gate, probes only Z at "
        "0.1 mm/s, removes OUT5 power, and rapidly returns to machine Z home"
    )


def validate_homing_style_tool_height_test() -> str:
    path = LIVE_DIR / "nc_files" / "tool-height-homing-style-test.ngc"
    raw_program = path.read_text(encoding="utf-8")
    program = executable_gcode_text(path)
    helper = (
        ROOT / "reference" / "python" / "dmc2_reference" / "probe_confirmation.py"
    ).read_text(encoding="utf-8")

    required = (
        "#<puck_machine_x> = 288.125",
        "#<puck_machine_y> = 152.955",
        "#<z_home_machine> = 135.0",
        "#<fast_probe_feed_mm_min> = 300.0",
        "#<backoff_distance_mm> = 1.0",
        "#<backoff_feed_mm_min> = 15.0",
        "#<slow_probe_feed_mm_min> = 15.0",
        "#<connection_timeout_seconds> = 300.0",
        "ABS[#<_abs_z> - #<z_home_machine>]",
        "M5",
        "M0",
        "M64 P0",
        "M66 P0 L1 Q#<connection_timeout_seconds>",
        "#<connection_result> = #5399",
        "G38.2 Z#<probe_target_z> F#<fast_probe_feed_mm_min>",
        "#<first_probe_machine_z> = #5063",
        "G53 G1 Z#<backoff_machine_z> F#<backoff_feed_mm_min>",
        "M66 P0 L4 Q1.0",
        "G38.2 Z#<probe_target_z> F#<slow_probe_feed_mm_min>",
        "#<second_probe_machine_z> = #5063",
        "M65 P0",
        "M66 P1 L4 Q1.0",
        "G53 G0 Z#<z_home_machine>",
    )
    missing = [token for token in required if token not in program]
    if missing:
        raise AssertionError(f"homing-style tool-height test is incomplete: {missing}")

    required_messages = (
        "DMC2 CONNECTION TEST PASSED: IN0 contact detected and OUT5 removed",
        "DMC2 CONNECTION PASSED: OUT5 OFF; place the puck, then resume",
    )
    missing = [token for token in required_messages if token not in raw_program]
    if missing:
        raise AssertionError(
            f"homing-style tool-height operator messages are incomplete: {missing}"
        )

    gates = [match.start() for match in re.finditer(r"(?m)^M0$", program)]
    if len(gates) != 2:
        raise AssertionError(
            "homing-style tool-height test must contain exactly two operator gates"
        )
    connection_gate, measurement_gate = gates
    connection_power_on = program.index("M64 P0", connection_gate)
    connection_wait = program.index(
        "M66 P0 L1 Q#<connection_timeout_seconds>", connection_power_on
    )
    connection_power_off = program.index("M65 P0", connection_wait)
    measurement_power_on = program.index("M64 P0", measurement_gate)
    first_probe = program.index(
        "G38.2 Z#<probe_target_z> F#<fast_probe_feed_mm_min>",
        measurement_power_on,
    )
    backoff = program.index(
        "G53 G1 Z#<backoff_machine_z> F#<backoff_feed_mm_min>"
    )
    second_probe = program.index(
        "G38.2 Z#<probe_target_z> F#<slow_probe_feed_mm_min>"
    )
    power_off = program.index("M65 P0", second_probe)
    return_home = program.index("G53 G0 Z#<z_home_machine>")
    if not (
        connection_gate
        < connection_power_on
        < connection_wait
        < connection_power_off
        < measurement_gate
        < measurement_power_on
        < first_probe
        < backoff
        < second_probe
        < power_off
        < return_home
    ):
        raise AssertionError(
            "tool-check order must be connection gate, no-motion contact test, "
            "OUT5 off, placement gate, power, fast probe, backoff, slow probe, "
            "OUT5 off, return"
        )

    connection_stage = program[connection_gate:measurement_gate]
    if re.search(
        r"(?m)^\s*G(?:0?0|0?1|38\.2)\b", connection_stage, flags=re.IGNORECASE
    ):
        raise AssertionError("connection-test stage must contain no axis motion")

    if len(re.findall(r"\bG38\.2\b", program, flags=re.IGNORECASE)) != 2:
        raise AssertionError("homing-style test must contain exactly two probe moves")
    if re.search(r"\bG(?:0?0|0?1|38\.2)\b[^\n]*\b[XY]", program, flags=re.IGNORECASE):
        raise AssertionError("homing-style tool-height test must not command X or Y")
    if re.search(r"\bM[34]\b", program, flags=re.IGNORECASE):
        raise AssertionError("homing-style tool-height test starts the spindle")

    helper_required = (
        "EXPECTED_Z = 135.0",
        'heading="1. CONNECTION TEST — NO MOTION"',
        'start_label="START CONNECTION TEST — NO MOTION"',
        'self.connection_left_first_gate = False',
        "if not self.connection_left_first_gate:",
        'heading="2. CONNECTION PASSED — PLACE PUCK"',
        'start_label="START TOOL MEASURE"',
        "self.command.auto(linuxcnc.AUTO_RESUME)",
    )
    missing = [token for token in helper_required if token not in helper]
    if missing:
        raise AssertionError(f"two-stage tool-check popup is incomplete: {missing}")

    return (
        "homing-style tool-height test requires a no-motion IN0 connection pass, "
        "removes OUT5 before its placement gate, probes at 5 mm/s, backs off "
        "1.00 mm at 0.25 mm/s, re-touches at 0.25 mm/s, removes OUT5, and "
        "rapidly returns to Z home"
    )
