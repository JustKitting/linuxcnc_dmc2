"""Read the shared DMC2 operation catalog used by AXIS and dmc2ctl."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


CATALOG_MAGIC = "DMC2_OPERATION_CATALOG\t2"
CATALOG_COLUMNS = (
    "id",
    "kind",
    "label",
    "driver",
    "target",
    "ui_scope",
    "effects",
    "prerequisites",
    "ui_target",
)


@dataclass(frozen=True)
class Operation:
    id: str
    kind: str
    label: str
    driver: str
    target: str
    ui_scope: str
    effects: tuple[str, ...]
    prerequisites: tuple[str, ...]
    ui_target: str


def project_catalog_path(rcfile: str) -> Path:
    return Path(rcfile).resolve().parents[2] / "config" / "operations.tsv"


def default_catalog_path() -> Path:
    return Path(__file__).resolve().parents[2] / "config" / "operations.tsv"


def read_operations(path: Path) -> dict[str, Operation]:
    lines = path.read_text(encoding="utf-8").splitlines()
    if len(lines) < 2 or lines[0] != CATALOG_MAGIC:
        raise RuntimeError(f"Unsupported DMC2 operation catalog: {path}")
    if tuple(lines[1].split("\t")) != CATALOG_COLUMNS:
        raise RuntimeError(f"Invalid DMC2 operation catalog columns: {path}")

    operations: dict[str, Operation] = {}
    for line_number, line in enumerate(lines[2:], start=3):
        if not line or line.startswith("#"):
            continue
        values = line.split("\t")
        if len(values) != len(CATALOG_COLUMNS):
            raise RuntimeError(f"Invalid operation catalog row {line_number}: {path}")
        fields = dict(zip(CATALOG_COLUMNS, values, strict=True))
        operation = Operation(
            id=fields["id"],
            kind=fields["kind"],
            label=fields["label"],
            driver=fields["driver"],
            target=fields["target"],
            ui_scope=fields["ui_scope"],
            effects=tuple(value for value in fields["effects"].split(";") if value),
            prerequisites=tuple(
                value for value in fields["prerequisites"].split(";") if value
            ),
            ui_target=fields["ui_target"],
        )
        if operation.kind not in ("control", "program", "ui", "internal"):
            raise RuntimeError(
                f"Invalid operation kind at row {line_number}: {operation.kind!r}"
            )
        if not operation.ui_target:
            raise RuntimeError(f"Missing UI target at row {line_number}: {path}")
        if operation.id in operations:
            raise RuntimeError(f"Duplicate operation {operation.id}: {path}")
        operations[operation.id] = operation
    return operations
