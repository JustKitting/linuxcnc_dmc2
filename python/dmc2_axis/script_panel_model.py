"""Typed widget data for reusable scripts; no machine command channel."""

from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal, InvalidOperation
from enum import Enum
import json
import math
from pathlib import Path
import re

from .operation_catalog import Operation


class ParameterKind(Enum):
    POSITIVE_MM = "positive-mm"
    SIGNED_MM = "signed-mm"


@dataclass(frozen=True)
class Parameter:
    pin: str
    label: str
    kind: ParameterKind
    default: str
    increment: Decimal

    def parse(self, text: str) -> float:
        try:
            number = Decimal(text.strip())
            value = float(number)
        except (InvalidOperation, ValueError, OverflowError) as error:
            raise ValueError(f"{self.label}: enter a number in mm.") from error
        if not number.is_finite() or not math.isfinite(value):
            raise ValueError(f"{self.label}: enter a finite number in mm.")
        if self.kind is ParameterKind.POSITIVE_MM and value <= 0:
            raise ValueError(f"{self.label}: enter a value greater than zero.")
        return value

    def incremented(self, text: str, direction: int) -> str:
        try:
            current = Decimal(text.strip())
        except InvalidOperation as error:
            raise ValueError(f"{self.label}: enter a number before using + or -.") from error
        if not current.is_finite():
            raise ValueError(f"{self.label}: enter a finite number before using + or -.")
        result = current + self.increment * direction
        return format(result, "f")


@dataclass(frozen=True)
class PanelScript:
    key: str
    operation: Operation
    description: str
    parameters: tuple[Parameter, ...]
    requires_beginning: bool = False

    @property
    def valid_pin(self) -> str:
        return f"{self.key}-parameters-valid"

    def values(self, texts: dict[str, str]) -> dict[str, float]:
        return {parameter.pin: parameter.parse(texts[parameter.pin]) for parameter in self.parameters}


def read_panel_scripts(path: Path, operations: dict[str, Operation]) -> tuple[PanelScript, ...]:
    document = json.loads(path.read_text(encoding="utf-8"))
    if set(document) != {"version", "scripts"} or document["version"] != 1:
        raise ValueError(f"Unsupported Custom Scripts parameter catalog: {path}")
    scripts = []
    keys: set[str] = set()
    pins: set[str] = set()
    for row in document["scripts"]:
        if set(row) - {"requires_beginning"} != {"key", "operation", "description", "parameters"}:
            raise ValueError(f"Invalid Custom Scripts entry in {path}")
        requires_beginning = row.get("requires_beginning", False)
        if type(requires_beginning) is not bool:
            raise ValueError(f"Invalid beginning-of-program requirement in {path}")
        key = row["key"]
        if not re.fullmatch(r"[a-z][a-z0-9-]*", key) or key in keys:
            raise ValueError(f"Invalid or duplicate Custom Scripts key: {key!r}")
        keys.add(key)
        operation = operations[row["operation"]]
        if operation.kind != "program" or operation.driver != "linuxcnc.program" or operation.ui_scope != "custom-scripts":
            raise ValueError(f"Invalid Custom Scripts operation: {operation.id}")
        parameters = []
        for field in row["parameters"]:
            if set(field) != {"pin", "label", "kind", "default", "increment"}:
                raise ValueError(f"Invalid parameter definition for {operation.label}")
            parameter = Parameter(field["pin"], field["label"], ParameterKind(field["kind"]), field["default"], Decimal(field["increment"]))
            if not re.fullmatch(r"[a-z][a-z0-9-]*", parameter.pin) or parameter.pin in pins:
                raise ValueError(f"Invalid or duplicate parameter pin: {parameter.pin}")
            if not parameter.increment.is_finite() or parameter.increment <= 0:
                raise ValueError(f"Invalid increment for {parameter.label}")
            # Zero is an explicit unset default for required measurements.
            initial = Decimal(parameter.default)
            if not initial.is_finite() or not math.isfinite(float(initial)):
                raise ValueError(f"Invalid default for {parameter.label}")
            pins.add(parameter.pin)
            parameters.append(parameter)
        scripts.append(PanelScript(key, operation, row["description"], tuple(parameters), requires_beginning))
    if not scripts:
        raise ValueError("The Custom Scripts catalog is empty.")
    return tuple(scripts)
