"""Stock AXIS owns the Rust planner's data parameters; no machine commands."""
import re


def _create_bank(component, hal, path, prefix, magic):
    lines = path.read_text(encoding="utf-8").splitlines()
    if not lines or lines[0] != magic or lines[1:2] != ["sequence"]:
        raise ValueError("The probe plan data catalog is missing or incompatible.")
    fields = lines[1:]
    if len(set(fields)) != len(fields) or any(not re.fullmatch(r"[a-z][a-z-]*", name) for name in fields):
        raise ValueError("The probe plan data catalog has duplicate or invalid fields.")
    for field in fields:
        name = prefix + "-plan-" + field
        component.newparam(name, hal.HAL_FLOAT, hal.HAL_RW)
        component[name] = -1.0 if field == "sequence" else 0.0


def create_plan_parameters(component, hal, catalog):
    lines = catalog.read_text(encoding="utf-8").splitlines()
    if not lines or lines[0] != "DMC2_PROBE_PLAN_BANKS_V1":
        raise ValueError("The probe plan bank catalog is missing or incompatible.")
    seen = set()
    for line in lines[1:]:
        prefix, filename, magic = line.split("\t")
        if prefix in seen or not re.fullmatch(r"[a-z][a-z-]*", prefix) or filename != prefix + "-plan-fields.txt":
            raise ValueError("The probe plan bank catalog has an invalid or repeated bank.")
        seen.add(prefix)
        _create_bank(component, hal, catalog.parent / filename, prefix, magic)
