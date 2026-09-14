"""Stock AXIS owns the Rust planner's data parameters; no machine commands."""
import re


def create_block_plan_parameters(component, hal, path):
    lines = path.read_text(encoding="utf-8").splitlines()
    if not lines or lines[0] != "DMC2_BLOCK_PLAN_V1" or lines[1:2] != ["sequence"]:
        raise ValueError("The gauge-block plan data catalog is missing or incompatible.")
    fields = lines[1:]
    if len(set(fields)) != len(fields) or any(not re.fullmatch(r"[a-z][a-z-]*", name) for name in fields):
        raise ValueError("The gauge-block plan data catalog has duplicate or invalid fields.")
    for field in fields:
        name = "block-plan-" + field
        component.newparam(name, hal.HAL_FLOAT, hal.HAL_RW)
        component[name] = -1.0 if field == "sequence" else 0.0
