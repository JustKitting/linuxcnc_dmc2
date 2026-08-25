from __future__ import annotations

from dataclasses import dataclass


AXES = frozenset({"X", "Y", "Z", "4", "5", "N", "I"})
MULTIPLIERS = frozenset({"X1", "X10", "X100", "N", "I"})


class ProtocolError(ValueError):
    pass


@dataclass(frozen=True)
class PendantPacket:
    sequence: int
    milliseconds: int
    detent_count: int
    transition_count: int
    quadrature_errors: int
    latest_detent_signal: int
    axis: str
    multiplier: str
    deadman_held: bool
    estop_pressed: bool
    selector_valid: bool

    @property
    def cnc_axis_valid(self) -> bool:
        return self.selector_valid and self.axis in {"X", "Y", "Z"}


def _parse_bool(token: str, field: str) -> bool:
    if token == "0":
        return False
    if token == "1":
        return True
    raise ProtocolError(f"{field} must be 0 or 1, got {token!r}")


def parse_packet(line: str) -> PendantPacket:
    fields = line.strip().split(",")
    if len(fields) != 12:
        raise ProtocolError(f"expected 12 fields, got {len(fields)}")
    if fields[0] != "P3":
        raise ProtocolError(f"unsupported protocol marker {fields[0]!r}")

    try:
        sequence = int(fields[1], 10)
        milliseconds = int(fields[2], 10)
        detent_count = int(fields[3], 10)
        transition_count = int(fields[4], 10)
        quadrature_errors = int(fields[5], 10)
        latest_detent_signal = int(fields[6], 10)
    except ValueError as error:
        raise ProtocolError("numeric field is not an integer") from error

    if sequence < 0 or milliseconds < 0 or quadrature_errors < 0:
        raise ProtocolError("unsigned fields cannot be negative")
    if latest_detent_signal not in (-1, 0, 1):
        raise ProtocolError("latest_detent_signal must be -1, 0, or 1")
    axis = fields[7]
    multiplier = fields[8]
    if axis not in AXES:
        raise ProtocolError(f"unknown axis token {axis!r}")
    if multiplier not in MULTIPLIERS:
        raise ProtocolError(f"unknown multiplier token {multiplier!r}")

    return PendantPacket(
        sequence=sequence,
        milliseconds=milliseconds,
        detent_count=detent_count,
        transition_count=transition_count,
        quadrature_errors=quadrature_errors,
        latest_detent_signal=latest_detent_signal,
        axis=axis,
        multiplier=multiplier,
        deadman_held=_parse_bool(fields[9], "deadman_held"),
        estop_pressed=_parse_bool(fields[10], "estop_pressed"),
        selector_valid=_parse_bool(fields[11], "selector_valid"),
    )


def signed_int32_delta(current: int, previous: int) -> int:
    """Return current-previous with signed 32-bit wraparound handling."""
    return ((current - previous + (1 << 31)) % (1 << 32)) - (1 << 31)
