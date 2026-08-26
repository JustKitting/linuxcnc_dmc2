"""Source-locked validation of the stock AXIS error-reader handoff."""

from __future__ import annotations

import shutil
from pathlib import Path

from .paths import PROJECT_ROOT


PINNED_AXIS = (
    PROJECT_ROOT
    / "vendor"
    / "linuxcnc-2.9.10"
    / "src"
    / "emc"
    / "usr_intf"
    / "axis"
    / "scripts"
    / "axis.py"
)


def _body_after_shebang(path: Path) -> bytes:
    try:
        source = path.read_bytes()
    except OSError as error:
        raise AssertionError(f"cannot read AXIS source {path}: {error}") from error
    shebang, separator, body = source.partition(b"\n")
    if not separator or not shebang.startswith(b"#!"):
        raise AssertionError(f"AXIS source has no executable shebang: {path}")
    return body


def validate_axis_error_reader_handoff(
    *,
    pinned_axis: Path = PINNED_AXIS,
    installed_axis: Path | None = None,
) -> str:
    if installed_axis is None:
        executable = shutil.which("axis")
        if executable is None:
            raise AssertionError("stock AXIS executable is unavailable")
        installed_axis = Path(executable)

    pinned_body = _body_after_shebang(pinned_axis)
    installed_body = _body_after_shebang(installed_axis)
    if installed_body != pinned_body:
        raise AssertionError(
            "installed AXIS implementation differs from pinned LinuxCNC 2.9.10 source"
        )

    source = installed_body.decode("utf-8")
    constructor = "e = linuxcnc.error_channel()"
    user_command = "exec(compile(open(rcfile, \"rb\").read(), rcfile, 'exec'))"
    first_poll = "live_plotter.error_task()"
    if source.count(constructor) != 1:
        raise AssertionError("stock AXIS error-channel constructor changed")
    if source.count("error = e.poll()") != 2:
        raise AssertionError("stock AXIS error-channel polling implementation changed")
    if source.count(user_command) != 1:
        raise AssertionError("stock AXIS USER_COMMAND_FILE execution point changed")
    if source.count(first_poll) != 1:
        raise AssertionError("stock AXIS first error-poll invocation changed")
    constructor_offset = source.index(constructor)
    user_command_offset = source.index(user_command)
    first_poll_offset = source.index(first_poll)
    if not constructor_offset < user_command_offset < first_poll_offset:
        raise AssertionError(
            "stock AXIS no longer executes USER_COMMAND_FILE after constructing and before polling its error channel"
        )
    return (
        "installed AXIS exactly matches pinned 2.9.10 and transfers error-reader "
        "ownership before its first poll"
    )
