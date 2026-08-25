"""Layout-independent parsers and assertion helpers."""

from __future__ import annotations

import configparser
import re
import xml.etree.ElementTree as ET
from pathlib import Path


def executable_hal_text(*paths: Path) -> str:
    return "\\n".join(
        line.partition("#")[0]
        for path in paths
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.partition("#")[0].strip()
    )


def source_tree_text(root: Path, *suffixes: str) -> str:
    """Read a source module tree deterministically, independent of file layout."""
    paths = sorted(
        path
        for path in root.rglob("*")
        if path.is_file() and path.suffix in suffixes
    )
    if not paths:
        raise AssertionError(f"source tree is empty: {root}")
    return "\\n".join(path.read_text(encoding="utf-8") for path in paths)


def read_ini(path: Path) -> configparser.ConfigParser:
    config = configparser.ConfigParser(strict=False)
    with path.open(encoding="utf-8") as stream:
        config.read_file(stream)
    return config


def require_float(
    config: configparser.ConfigParser,
    section: str,
    key: str,
    expected: float,
) -> None:
    actual = config.getfloat(section, key)
    if actual != expected:
        raise AssertionError(
            f"{section}.{key} must be exactly {expected}, found {actual}"
        )


def require_text(
    config: configparser.ConfigParser,
    section: str,
    key: str,
    expected: str,
) -> None:
    actual = config.get(section, key)
    if actual != expected:
        raise AssertionError(
            f"{section}.{key} must be exactly {expected!r}, found {actual!r}"
        )


def pin_names_from_panel(path: Path) -> set[str]:
    tree = ET.parse(path)
    if tree.getroot().tag != "pyvcp":
        raise AssertionError("status panel root must be <pyvcp>")
    pins = set()
    for node in tree.iter():
        attribute = node.attrib.get("halpin")
        if attribute is not None:
            pins.add(attribute.strip().strip('"').strip("'"))
    for node in tree.findall(".//halpin"):
        if node.text is None:
            raise AssertionError("empty <halpin> in status panel")
        pins.add(node.text.strip().strip('"'))
    return pins


def pin_names_from_postgui(path: Path) -> set[str]:
    text = path.read_text(encoding="utf-8")
    return set(re.findall(r"pyvcp\.([a-z0-9-]+)", text))
