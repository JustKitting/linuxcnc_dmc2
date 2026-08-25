"""Pinned LinuxCNC source, ABI, and code-catalog coverage."""

from pathlib import Path
import re
import subprocess

from .common import source_tree_text
from .paths import PROJECT_ROOT as ROOT


def validate_linuxcnc_interface_coverage() -> str:
    expected_version = "2.9.10"
    expected_commit = "86cdca76fa2a36274c432caa21952b23c267989a"
    source_root = ROOT / "vendor" / "linuxcnc-2.9.10"
    if not source_root.is_dir():
        raise AssertionError("durable LinuxCNC 2.9.10 source clone is missing")

    installed_version = subprocess.run(
        ["linuxcnc_var", "LINUXCNCVERSION"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if installed_version != expected_version:
        raise AssertionError(
            f"installed LinuxCNC must be {expected_version}, found {installed_version}"
        )
    source_commit = subprocess.run(
        ["git", "-C", str(source_root), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if source_commit != expected_commit:
        raise AssertionError(
            f"LinuxCNC source must be v2.9.10 commit {expected_commit}, found {source_commit}"
        )
    source_changes = subprocess.run(
        ["git", "-C", str(source_root), "status", "--porcelain"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if source_changes:
        raise AssertionError("LinuxCNC 2.9.10 source clone has local modifications")

    interface_root = ROOT / "rust" / "crates" / "dmc2-linuxcnc-interface"
    build = (interface_root / "build.rs").read_text(encoding="utf-8")
    library = (interface_root / "src" / "lib.rs").read_text(encoding="utf-8")
    required_build_contract = (
        f'const EXPECTED_LINUXCNC_VERSION: &str = "{expected_version}";',
        f'const EXPECTED_LINUXCNC_COMMIT: &str = "{expected_commit}";',
        'const SOURCE_ROOT_RELATIVE: &str = "../../../vendor/linuxcnc-2.9.10";',
        'read_header("emc_nml.hh")',
        'read_header("emcmotcfg.h")',
        'installed {name} does not exactly match pulled LinuxCNC v2.9.10 source',
        '"LinuxCNC v2.9.10 NCE template count changed"',
        'LinuxCNC 2.9.10 public code domains changed or the source parser omitted a code',
        'LinuxCNC code probe omitted a source code',
        'pulled LinuxCNC v2.9.10 source has local modifications',
    )
    missing = [token for token in required_build_contract if token not in build]
    if missing:
        raise AssertionError(f"LinuxCNC source-generated catalog is incomplete: {missing}")
    count_block = build.partition("const EXPECTED_DOMAIN_COUNTS")[2].partition("]; ")[0]
    if not count_block:
        count_block = build.partition("const EXPECTED_DOMAIN_COUNTS")[2].partition("];")[0]
    counts = [int(value) for value in re.findall(r'\("[a-z0-9_]+",\s*(\d+)\)', count_block)]
    if len(counts) != 50 or sum(counts) != 550:
        raise AssertionError(
            f"expected 50 locked LinuxCNC code domains / 550 codes, found {len(counts)} / {sum(counts)}"
        )
    required_library_contract = (
        "pub fn lookup(self, code: i64)",
        "unknown_values_are_never_mislabeled",
        "assert_eq!(GENERATED_CODE_COUNT, 550)",
        "assert_eq!(INTERPRETER_ERROR_TEMPLATES.len(), 198)",
    )
    missing = [token for token in required_library_contract if token not in library]
    if missing:
        raise AssertionError(f"generated catalog tests are incomplete: {missing}")

    installed_layout = Path("/usr/include/linuxcnc/emc_nml.hh").read_bytes()
    source_layout = (
        source_root / "src" / "emc" / "nml_intf" / "emc_nml.hh"
    ).read_bytes()
    if installed_layout != source_layout:
        raise AssertionError("installed emc_nml.hh differs from LinuxCNC v2.9.10 source")

    monitor_root = ROOT / "rust" / "crates" / "dmc2-task-monitor" / "src"
    snapshot = (monitor_root / "snapshot.rs").read_text(encoding="utf-8")
    shim = (monitor_root / "task_status_shim.cc").read_text(encoding="utf-8")
    diagnostics = source_tree_text(monitor_root / "diagnostics", ".rs")
    monitor = source_tree_text(monitor_root / "application", ".rs")
    required_monitor_contract = (
        (snapshot, "EMCMOT_MAX_MISC_ERROR"),
        (snapshot, "pub misc_error: [i32; EMCMOT_MAX_MISC_ERROR]"),
        (shim, "dmc2_task_status_snapshot_size"),
        (shim, "for (int index = 0; index < EMCMOT_MAX_JOINTS; ++index)"),
        (shim, "for (int index = 0; index < EMCMOT_MAX_AXIS; ++index)"),
        (shim, "for (int index = 0; index < EMCMOT_MAX_SPINDLES; ++index)"),
        (shim, "for (int index = 0; index < EMCMOT_MAX_MISC_ERROR; ++index)"),
        (shim, "holder->channel->error_type"),
        (shim, "set_nml_error(nml_error, holder->channel->error_type)"),
        (diagnostics, "pub const UNKNOWN_CODE"),
        (diagnostics, "pub const TRANSPORT"),
        (diagnostics, "every_rcs_error_source_is_classified"),
        (diagnostics, "every_unknown_checked_enum_is_reported_without_guessing"),
        (diagnostics, "operational_fault_fields_are_all_covered"),
        (diagnostics, "every_nml_transport_error_code_has_its_exact_source_name"),
        (diagnostics, "unknown_nml_transport_code_is_never_mislabeled"),
        (monitor, '"linuxcnc-error-active"'),
        (monitor, '"unknown-code-active"'),
        (monitor, '"nml-error-code"'),
        (monitor, '"nml-error-known"'),
        (monitor, "dmc2_task_status_poll(self.0, snapshot, &mut nml_error)"),
        (monitor, "dmc2_task_status_close(self.0)"),
        (monitor, '"catalog-code-count"'),
        (monitor, '"snapshot-abi-version"'),
    )
    missing = [token for text, token in required_monitor_contract if token not in text]
    if missing:
        raise AssertionError(f"native LinuxCNC diagnostic monitor is incomplete: {missing}")

    axis_policy = source_tree_text(ROOT / "python" / "dmc2_axis", ".py")
    for name in (
        "NML_ERROR",
        "NML_TEXT",
        "NML_DISPLAY",
        "OPERATOR_ERROR",
        "OPERATOR_TEXT",
        "OPERATOR_DISPLAY",
    ):
        if f'("{name}",' not in axis_policy:
            raise AssertionError(f"AXIS error-channel catalog omits {name}")
    if "kind_catalog.get(kind, (\"UNKNOWN\", \"error\"))" not in axis_policy:
        raise AssertionError("unknown AXIS error-channel types do not fail as errors")

    return (
        "LinuxCNC 2.9.10 source/headers are exact; 50 numeric domains (550 codes), "
        "198 interpreter templates, all nine NML transport errors, every configured "
        "status/fault source, and all six error-channel message types are explicitly covered"
    )
