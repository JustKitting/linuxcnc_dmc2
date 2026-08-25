"""H100 integration and parameterized spindle-operation validation."""

import re
import struct
import tomllib
import xml.etree.ElementTree as ET
from pathlib import Path

from .paths import LIVE_DIR, PROJECT_ROOT as ROOT


H100_INTERFACE_NAMES = {
    "h100-spindle.machine-enabled",
    "h100-spindle.run-request",
    "h100-spindle.forward-request",
    "h100-spindle.reverse-request",
    "h100-spindle.speed-command-rpm",
    "h100-spindle.reset",
    "h100-spindle.link-fault",
    *(f"h100-spindle.command-disabled{index}" for index in range(13)),
    "h100-spindle.control-mode-f001",
    "h100-spindle.frequency-source-f002",
    "h100-spindle.reference-f004-centihz",
    "h100-spindle.maximum-f005-centihz",
    "h100-spindle.lower-limit-f011-centihz",
    "h100-spindle.panel-stop-f024",
    "h100-spindle.slave-address-f163",
    "h100-spindle.baud-selector-f164",
    "h100-spindle.data-mode-f165",
    "h100-spindle.frequency-decimals-f169",
    "h100-spindle.output-frequency-decihz",
    "h100-spindle.current-fault",
    "h100-spindle.main-status",
    "h100-spindle.given-frequency-readback",
    "h100-spindle.main-control",
    "h100-spindle.given-frequency",
    "h100-spindle.fault-code",
    "h100-spindle.block-code",
    "h100-spindle.state",
    "h100-spindle.ready",
    "h100-spindle.running",
    "h100-spindle.forward-running",
    "h100-spindle.reverse-running",
    "h100-spindle.at-speed",
    "h100-spindle.fault-latched",
    "h100-spindle.speed-feedback-rpm",
    "h100-spindle.target-frequency-hz",
    "h100-spindle.rated-rpm",
    "h100-spindle.minimum-rpm",
    "h100-spindle.maximum-rpm",
    "h100-spindle.expected-reference-f004-centihz",
    "h100-spindle.expected-maximum-f005-centihz",
    "h100-spindle.at-speed-tolerance-hz",
}


def _require_tokens(source: str, description: str, tokens: tuple[str, ...]) -> None:
    missing = [token for token in tokens if token not in source]
    if missing:
        raise AssertionError(f"{description} is incomplete: {missing}")


def _validate_h100_release(path: Path) -> None:
    header = path.read_bytes()[:20]
    if len(header) < 20 or header[:4] != b"\x7fELF":
        raise AssertionError("H100 release is not an ELF binary")
    if header[4] != 2 or header[5] != 1:
        raise AssertionError("H100 release must be little-endian ELF64")
    elf_type, machine = struct.unpack_from("<HH", header, 16)
    if elf_type != 3:
        raise AssertionError("H100 release is not an ELF shared object")
    if machine != 183:
        raise AssertionError("H100 release is not built for AArch64")


def validate_h100_spindle_integration() -> str:
    project = ROOT.parent / "h100_modbus"
    map_path = project / "maps" / "live" / "h100-spindle.mbccs"
    binary_path = project / "maps" / "live" / "h100-spindle.mbccb"
    rust_root = project / "rust"
    crate = rust_root / "crates" / "h100-spindle"
    source_paths = {
        "library": crate / "src" / "lib.rs",
        "component": crate / "src" / "component" / "mod.rs",
        "pins": crate / "src" / "component" / "pins.rs",
        "registration": crate / "src" / "component" / "registration.rs",
        "cycle": crate / "src" / "component" / "cycle.rs",
        "component tests": crate / "src" / "component" / "tests" / "mod.rs",
        "sequencer": crate / "src" / "sequencer" / "step.rs",
        "sequencer types": crate / "src" / "sequencer" / "types.rs",
        "sequencer tests": crate / "src" / "sequencer" / "tests.rs",
    }
    release_path = project / "target" / "release" / "h100_spindle.so"
    required_paths = (
        map_path,
        binary_path,
        rust_root / "Cargo.toml",
        rust_root / "Cargo.lock",
        crate / "Cargo.toml",
        crate / "build.rs",
        release_path,
        *source_paths.values(),
    )
    for path in required_paths:
        if not path.is_file():
            raise AssertionError(f"H100 integration artifact is missing: {path}")

    legacy_paths = (
        project / "src" / "component" / "h100_spindle.comp",
        project / "src" / "component" / "h100_spindle_logic.c",
        project / "src" / "component" / "h100_spindle_logic.h",
    )
    present_legacy_paths = [str(path) for path in legacy_paths if path.exists()]
    if present_legacy_paths:
        raise AssertionError(
            f"legacy H100 C implementation is still present: {present_legacy_paths}"
        )

    workspace_manifest = tomllib.loads(
        (rust_root / "Cargo.toml").read_text(encoding="utf-8")
    )
    crate_manifest = tomllib.loads(
        (crate / "Cargo.toml").read_text(encoding="utf-8")
    )
    lockfile = tomllib.loads(
        (rust_root / "Cargo.lock").read_text(encoding="utf-8")
    )
    if workspace_manifest.get("workspace", {}).get("members") != [
        "crates/h100-spindle"
    ]:
        raise AssertionError("H100 Rust workspace does not contain exactly the live crate")
    if crate_manifest.get("package", {}).get("name") != "h100-spindle":
        raise AssertionError("H100 live Rust crate has the wrong package name")
    if set(crate_manifest.get("lib", {}).get("crate-type", [])) != {
        "cdylib",
        "rlib",
    }:
        raise AssertionError("H100 Rust crate must produce a cdylib and testable rlib")
    hal_dependency = crate_manifest.get("dependencies", {}).get("dmc2-hal-sys", {})
    if hal_dependency.get("path") != (
        "../../../../linuxcnc_dmc2/rust/crates/dmc2-hal-sys"
    ):
        raise AssertionError("H100 Rust crate does not use the verified LinuxCNC HAL ABI")
    locked_packages = {package.get("name") for package in lockfile.get("package", [])}
    if locked_packages != {"dmc2-hal-sys", "h100-spindle"}:
        raise AssertionError(f"unexpected H100 Rust lockfile packages: {locked_packages}")

    _validate_h100_release(release_path)

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

    sources = {
        name: path.read_text(encoding="utf-8")
        for name, path in source_paths.items()
    }
    observed_interface = set(
        re.findall(
            r'"(h100-spindle\.[a-z0-9-]+)(?:\\0)?"',
            sources["registration"],
        )
    )
    if observed_interface != H100_INTERFACE_NAMES:
        raise AssertionError(
            "H100 Rust HAL interface changed: "
            f"missing={sorted(H100_INTERFACE_NAMES - observed_interface)}, "
            f"unexpected={sorted(observed_interface - H100_INTERFACE_NAMES)}"
        )

    _require_tokens(
        sources["component"],
        "H100 Rust HAL lifecycle",
        (
            'const COMPONENT_NAME: &[u8] = b"h100_spindle\\0";',
            'const FUNCTION_NAME: &[u8] = b"h100-spindle\\0";',
            'pub extern "C" fn rtapi_app_main() -> c_int',
            'pub extern "C" fn rtapi_app_exit()',
            "hal::hal_init(",
            "hal::hal_malloc(",
            "register_interface(component, component_id)?;",
            "publish_initial_values(&mut *component);",
            "hal::hal_export_funct(",
            "hal::hal_ready(component_id)",
            "exit_component(component_id);",
        ),
    )
    _require_tokens(
        sources["registration"],
        "H100 Rust safe startup",
        (
            "write(pins.link_fault, true);",
            "write(pins.main_control, CONTROL_STOP);",
            "write(pins.given_frequency, 0);",
            "write(pins.ready, false);",
            "write(pins.at_speed, true);",
            "write(pins.fault_latched, false);",
            "write(pins.state, State::Stopped as u32);",
        ),
    )
    _require_tokens(
        sources["cycle"],
        "H100 Rust realtime cycle",
        (
            "let input = Input {",
            "any_command_disabled: pins",
            ".any(|pointer| unsafe { read(*pointer) }),",
            "let output = component.context.step(input, config);",
            "write(pins.main_control, output.main_control);",
            "write(pins.given_frequency, output.given_frequency);",
            "write(pins.fault_latched, output.fault_latched);",
            "write(pins.block_code, output.block_code);",
            "write(pins.state, output.state);",
        ),
    )
    _require_tokens(
        sources["sequencer types"] + "\n" + sources["sequencer"],
        "H100 Rust fail-stopped sequencer",
        (
            "pub const CONTROL_FORWARD: u32 = 0x0001;",
            "pub const CONTROL_REVERSE: u32 = 0x0004;",
            "pub const CONTROL_STOP: u32 = 0x0008;",
            "CommandDisabled = 22,",
            "InternalState = 23,",
            "main_control: CONTROL_STOP,",
            "if input.given_frequency_readback == self.held_frequency",
            "self.latch(run_reason);",
            "if self.state().active() && base_reason != BlockCode::None",
            "State::Stopping =>",
            "State::Fault =>",
            "output.main_control = CONTROL_STOP;",
            "if stopped_feedback",
        ),
    )
    _require_tokens(
        sources["component tests"] + "\n" + sources["sequencer tests"],
        "H100 Rust program tests",
        (
            "const PIN_COUNT: usize = 47;",
            "const PARAMETER_COUNT: usize = 6;",
            "fn exact_hal_schema_and_initial_values_are_preserved()",
            "fn hal_inputs_drive_the_exact_start_run_speed_change_and_stop_sequence()",
            "fn every_modbus_disabled_input_blocks_and_faults_the_run_request()",
            "fn every_lifecycle_failure_is_returned_and_cleaned_up()",
            "fn every_configuration_block_code_and_priority_is_exact()",
            "fn every_run_refusal_code_has_an_exact_trigger()",
            "fn configuration_faults_latch_from_every_active_state_and_preserve_stop()",
            "fn exhaustive_boolean_state_matrix_preserves_all_output_invariants()",
            "assert_eq!(cases, 122_880);",
        ),
    )

    return (
        "H100 map starts suspended, writes STOP/control before frequency, reads "
        "all run prerequisites, and uses the native Rust HAL component with "
        "the tested fail-stopped sequencer release"
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
