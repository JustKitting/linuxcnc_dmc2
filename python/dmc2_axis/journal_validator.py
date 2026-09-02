"""Command-line validation of the exact AXIS error-journal reader."""

from __future__ import annotations

import argparse
from collections.abc import Mapping
from pathlib import Path

from .error_journal import ErrorJournalReader
from .notifications import error_channel_kind_catalog, should_suppress_notification


def validate(
    path: Path,
    minimum_events: int,
    maximum_events: int | None = None,
    only_message: str | None = None,
    forbidden_substrings: tuple[str, ...] = (),
    required_message_counts: Mapping[str, int] | None = None,
    suppression_module=None,
) -> int:
    required_message_counts = required_message_counts or {}
    if suppression_module is not None:
        error_channel_kind_catalog(suppression_module)
    reader = ErrorJournalReader(path)
    count = 0
    observed_message_counts = dict.fromkeys(required_message_counts, 0)
    try:
        while (event := reader.poll()) is not None:
            count += 1
            if only_message is not None and event.display_text() != only_message:
                raise RuntimeError(
                    "error journal contained an unexpected LinuxCNC event: "
                    f"sequence={event.sequence}, expected={only_message!r}, "
                    f"observed={event.display_text()!r}, path={path}"
                )
            for forbidden in forbidden_substrings:
                if forbidden in event.display_text():
                    raise RuntimeError(
                        "error journal contained a forbidden LinuxCNC event: "
                        f"sequence={event.sequence}, forbidden={forbidden!r}, "
                        f"observed={event.display_text()!r}, path={path}"
                    )
            message = event.display_text()
            if message in observed_message_counts:
                observed_message_counts[message] += 1
                if suppression_module is not None and not should_suppress_notification(
                    int(event.message_type), message, suppression_module
                ):
                    raise RuntimeError(
                        "required controller event is visible under the production "
                        "AXIS policy: "
                        f"sequence={event.sequence}, type={event.message_type}, "
                        f"observed={message!r}, path={path}"
                    )
    finally:
        reader.close()
    if count < minimum_events:
        raise RuntimeError(
            "error journal contained fewer complete LinuxCNC events than required: "
            f"minimum={minimum_events}, observed={count}, path={path}"
        )
    if maximum_events is not None and count > maximum_events:
        raise RuntimeError(
            "error journal contained more complete LinuxCNC events than allowed: "
            f"maximum={maximum_events}, observed={count}, path={path}"
        )
    for message, expected in required_message_counts.items():
        observed = observed_message_counts[message]
        if observed != expected:
            raise RuntimeError(
                "error journal controller-event count mismatch: "
                f"message={message!r}, expected={expected}, observed={observed}, "
                f"path={path}"
            )
    return count


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("journal", type=Path)
    parser.add_argument("--minimum-events", type=int, default=0)
    parser.add_argument("--maximum-events", type=int)
    parser.add_argument("--only-message")
    parser.add_argument("--forbid-substring", action="append", default=[])
    parser.add_argument(
        "--require-message-count",
        action="append",
        nargs=2,
        metavar=("MESSAGE", "COUNT"),
        default=[],
    )
    parser.add_argument(
        "--required-messages-must-be-suppressed",
        action="store_true",
    )
    arguments = parser.parse_args()
    if arguments.minimum_events < 0:
        parser.error("--minimum-events must be nonnegative")
    if arguments.maximum_events is not None:
        if arguments.maximum_events < arguments.minimum_events:
            parser.error("--maximum-events must be at least --minimum-events")
    required_message_counts = {}
    for message, raw_count in arguments.require_message_count:
        try:
            expected_count = int(raw_count)
        except ValueError:
            parser.error(f"required message count must be an integer: {raw_count!r}")
        if expected_count < 0:
            parser.error("required message counts must be nonnegative")
        if message in required_message_counts:
            parser.error(f"required message was specified more than once: {message!r}")
        required_message_counts[message] = expected_count
    suppression_module = None
    if arguments.required_messages_must_be_suppressed:
        if not required_message_counts:
            parser.error(
                "--required-messages-must-be-suppressed requires "
                "--require-message-count"
            )
        import linuxcnc

        suppression_module = linuxcnc
    count = validate(
        arguments.journal,
        arguments.minimum_events,
        arguments.maximum_events,
        arguments.only_message,
        tuple(arguments.forbid_substring),
        required_message_counts,
        suppression_module,
    )
    print(f"DMC2_ERROR_JOURNAL_VALID events={count}")


if __name__ == "__main__":
    main()
