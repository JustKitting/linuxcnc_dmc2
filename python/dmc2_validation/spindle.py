"""H100 integration and parameterized spindle-operation validation."""

import re
import xml.etree.ElementTree as ET
from pathlib import Path

from .paths import LIVE_DIR, PROJECT_ROOT as ROOT


def validate_h100_spindle_integration() -> str:
    project = ROOT.parent / "h100_modbus"
    map_path = project / "h100-spindle.mbccs"
    binary_path = project / "h100-spindle.mbccb"
    component_path = project / "h100_spindle.comp"
    logic_path = project / "h100_spindle_logic.h"
    test_path = project / "test_h100_spindle_logic.c"
    for path in (map_path, binary_path, component_path, logic_path, test_path):
        if not path.is_file():
            raise AssertionError(f"H100 integration artifact is missing: {path}")

    tree = ET.parse(map_path)
    root = tree.getroot()
    if root.attrib.get("suspend") != "true":
        raise AssertionError("H100 Modbus map must start suspended")
    if root.attrib.get("writeflush") != "false":
        raise AssertionError("H100 Modbus writes must transmit established STOP state")

    writes = [
        int(command.attrib["address"], 0)
        for command in root.findall("./commands/command")
        if command.attrib.get("function") == "W_REGISTER"
    ]
    if writes != [0x0200, 0x0201]:
        raise AssertionError(
            "H100 write order must be main-control STOP/RUN before frequency"
        )

    read_addresses = {
        int(command.attrib["address"], 0)
        for command in root.findall("./commands/command")
        if command.attrib.get("function") in {"R_REGISTERS", "R_INPUTREGS"}
    }
    required_reads = {
        0x0001, 0x0004, 0x000B, 0x000E, 0x0018, 0x00A3, 0x00A9,
        0x0201, 0x0210,
    }
    if not required_reads <= read_addresses:
        raise AssertionError(
            f"H100 prerequisite/readback registers missing: "
            f"{sorted(required_reads - read_addresses)}"
        )

    input_register_commands = [
        command
        for command in root.findall("./commands/command")
        if command.attrib.get("function") == "R_INPUTREGS"
        and int(command.attrib["address"], 0) == 0x0000
    ]
    if len(input_register_commands) != 1:
        raise AssertionError("H100 map must have one input-register status read")
    status_pin_names = [
        pin.attrib["name"] for pin in input_register_commands[0].findall("pin")
    ]
    if status_pin_names != [
        "output-frequency", "set-frequency", "output-current"
    ]:
        raise AssertionError(
            "H100 status read must preserve the proven 0000H..0002H span"
        )

    component = component_path.read_text(encoding="utf-8")
    logic = logic_path.read_text(encoding="utf-8")
    required_tokens = (
        'pin out u32 main_control = 8',
        'pin out u32 given_frequency = 0',
        '#include "h100_spindle_logic.h"',
        "H100_CONTROL_REVERSE",
        "H100_BLOCK_COMMAND_DISABLED",
        "input.i_any_command_disabled = 1;",
        "input->i_given_frequency_readback ==",
        "case H100_STOPPING:",
        "output->o_main_control = H100_CONTROL_STOP;",
        "if (stopped_feedback)",
    )
    combined_source = component + "\n" + logic
    missing = [token for token in required_tokens if token not in combined_source]
    if missing:
        raise AssertionError(f"H100 fail-stopped sequencer is incomplete: {missing}")

    return (
        "H100 map starts suspended, writes STOP/control before frequency, reads "
        "all run prerequisites, and uses the unit-tested fail-stopped sequencer"
    )


def executable_gcode_text(path: Path) -> str:
    text = path.read_text(encoding="utf-8")
    text = re.sub(r"\([^)]*\)", " ", text, flags=re.DOTALL)
    return "\n".join(line.partition(";")[0] for line in text.splitlines())


def validate_spindle_test_operation() -> str:
    path = LIVE_DIR / "nc_files" / "dmc2_spindle_test.ngc"
    raw = path.read_text(encoding="utf-8")
    program = executable_gcode_text(path)

    required_tokens = (
        "o<dmc2_spindle_test> sub",
        "#<test_rpm> = #1",
        "#<transition_timeout_seconds> = #<_ini[dmc2]spindle_test_transition_timeout_seconds>",
        "#<_ini[dmc2]spindle_minimum_rpm>",
        "#<_ini[dmc2]spindle_maximum_rpm>",
        "S#<test_rpm> M3",
        "M66 P3 L3 Q#<transition_timeout_seconds>",
        "M66 P2 L3 Q#<transition_timeout_seconds>",
        "M66 P3 L4 Q#<transition_timeout_seconds>",
        "o<dmc2_spindle_test> endsub",
    )
    missing = [token for token in required_tokens if token not in raw]
    if missing:
        raise AssertionError(f"parameterized spindle test is incomplete: {missing}")

    if re.search(r"(?i)\bS\s*[-+]?\d", program):
        raise AssertionError("spindle test contains a fixed numeric S command")
    if len(re.findall(r"(?i)\bM3\b", program)) != 1:
        raise AssertionError("spindle test must contain exactly one clockwise M3")
    if re.search(r"(?im)^\s*G(?:0|1|2|3|38(?:\.\d+)?)\b", program):
        raise AssertionError("spindle test contains an axis-motion G-code")
    if re.search(r"(?i)\bM4\b", program):
        raise AssertionError("spindle test changes the verified clockwise direction")
    if program.upper().count("M5") < 4:
        raise AssertionError("every spindle-test failure path is not fail-stopped")

    return (
        "named spindle test takes RPM as #1, uses direct M3/M5, waits for "
        "H100 running/at-speed/stopped feedback, and contains no axis motion"
    )
