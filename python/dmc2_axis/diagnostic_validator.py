"""Command-line validation of the exact AXIS diagnostic-journal reader."""

from __future__ import annotations

import argparse
from pathlib import Path

from .diagnostic_journal import DiagnosticJournalReader


def validate(path: Path, forbidden_sources: frozenset[str]) -> int:
    reader = DiagnosticJournalReader(path)
    count = 0
    try:
        while (event := reader.poll()) is not None:
            count += 1
            if event.source in forbidden_sources:
                raise RuntimeError(
                    "diagnostic journal contained a forbidden source: "
                    f"sequence={event.sequence}, source={event.source!r}, "
                    f"identity={event.identity!r}, path={path}"
                )
    finally:
        reader.close()
    return count


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("journal", type=Path)
    parser.add_argument("--forbid-source", action="append", default=[])
    arguments = parser.parse_args()
    count = validate(arguments.journal, frozenset(arguments.forbid_source))
    print(f"DMC2_DIAGNOSTIC_JOURNAL_VALID events={count}")


if __name__ == "__main__":
    main()
