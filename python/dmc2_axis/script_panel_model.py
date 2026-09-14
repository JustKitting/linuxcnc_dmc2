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
    default_reference: str | None = None

    def initial_text(self, preferred: str) -> str:
        if self.default_reference is None or self.kind is not ParameterKind.POSITIVE_MM:
            return preferred
        try:
            unset = Decimal(preferred.strip()).is_zero()
        except InvalidOperation:
            # Preserve invalid text for the existing visible field validation.
            return preferred
        # A saved zero was an unset field, not an operator measurement.
        return self.default if unset else preferred

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
    if set(document) - {"defaults"} != {"version", "scripts"} or document["version"] != 1:
        raise ValueError(f"Unsupported Custom Scripts parameter catalog: {path}")
    defaults: dict[str, str] = {}
    definitions = document.get("defaults", {})
    if not isinstance(definitions, dict):
        raise ValueError(f"Custom Scripts shared defaults must be named definitions: {path}")
    for reference, definition in definitions.items():
        if not re.fullmatch(r"[a-z][a-z0-9-]*", reference):
            raise ValueError(f"Invalid shared default name: {reference!r}")
        if not isinstance(definition, dict) or set(definition) != {"value", "description", "source"}:
            raise ValueError(f"Shared default {reference}: provide value, description and source in {path}")
        if not all(isinstance(value, str) and value.strip() for value in definition.values()):
            raise ValueError(f"Shared default {reference}: value, description and source must be nonempty text in {path}")
        try:
            initial = Decimal(definition["value"])
        except InvalidOperation as error:
            raise ValueError(f"Shared default {reference}: enter a numeric value in {path}") from error
        if not initial.is_finite() or not math.isfinite(float(initial)):
            raise ValueError(f"Shared default {reference}: enter a finite value in {path}")
        defaults[reference] = definition["value"]
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
            initial_text = field["default"]
            reference = None
            if isinstance(initial_text, dict) and set(initial_text) == {"reference"}:
                reference = initial_text["reference"]
                if not isinstance(reference, str) or reference not in defaults:
                    raise ValueError(f"{field['label']}: unknown shared default {reference!r}; correct the reference in {path}")
                initial_text = defaults[reference]
            if not isinstance(initial_text, str):
                raise ValueError(f"{field['label']}: default must be numeric text or a named reference in {path}")
            parameter = Parameter(field["pin"], field["label"], ParameterKind(field["kind"]), initial_text, Decimal(field["increment"]), reference)
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
