"""Create AXIS data parameters from the Rust calibration type declaration."""
import re
import subprocess
from .script_contract import INSPECTION_TIMEOUT_SECONDS


def create_calibration_parameters(component, hal, binary):
    result = subprocess.run(
        [str(binary), "--describe-calibration-data"],
        check=True, capture_output=True, text=True, timeout=INSPECTION_TIMEOUT_SECONDS,
    )
    lines = result.stdout.splitlines()
    if not lines or lines[0] != "DMC2_CALIBRATION_DATA_V1":
        raise ValueError("Calibration data contract is incompatible; reopen the matching application.")
    types = {"bit": hal.HAL_BIT, "float": hal.HAL_FLOAT}
    fields = []
    names = set()
    for line in lines[1:]:
        name, kind, initial = line.split("\t")
        if not re.fullmatch(r"[a-z][a-z-]*", name) or name in names or kind not in types or initial != "0":
            raise ValueError("Calibration data declaration is invalid; reopen the matching application.")
        names.add(name)
        fields.append((name, types[kind]))
    if not fields:
        raise ValueError("Calibration data declaration is empty; reopen the matching application.")
    for name, kind in fields:
        component.newparam(name, kind, hal.HAL_RW)
        component[name] = 0
