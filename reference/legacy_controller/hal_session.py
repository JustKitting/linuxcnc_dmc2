from __future__ import annotations

import os
import pty
import subprocess
import select
import termios
import time
from collections.abc import Iterable

from hal_topology import HalCommand


class HalSessionError(RuntimeError):
    pass


def _render_hal_command(command: HalCommand) -> str:
    rendered: list[str] = []
    for argument in command:
        if argument in {"=>", "<=", "<=>"}:
            rendered.append(argument)
        elif any(character.isspace() for character in argument):
            escaped = argument.replace("\\", "\\\\").replace('"', '\\"')
            rendered.append(f'"{escaped}"')
        else:
            rendered.append(argument)
    return " ".join(rendered)


class HalSession:
    """Own one halrun instance while issuing checked halcmd commands."""

    def __init__(self) -> None:
        self._process: subprocess.Popen[str] | None = None
        self._master_fd: int | None = None
        self._command_number = 0
        self._output: list[str] = []

    def start(self) -> None:
        if self._process is not None:
            raise HalSessionError("HAL session was already started")
        master_fd, slave_fd = pty.openpty()
        attributes = termios.tcgetattr(slave_fd)
        attributes[3] &= ~termios.ECHO
        termios.tcsetattr(slave_fd, termios.TCSANOW, attributes)
        try:
            self._process = subprocess.Popen(
                ["halrun", "-s"],
                stdin=slave_fd,
                stdout=slave_fd,
                stderr=slave_fd,
                close_fds=True,
                # The parent controller receives Ctrl-C from its own terminal.
                # Keep halrun out of that signal group so the parent's finally
                # block can still send stop/unloadrt/quit in order.
                start_new_session=True,
            )
        finally:
            os.close(slave_fd)
        self._master_fd = master_fd
        attributes = termios.tcgetattr(master_fd)
        attributes[3] &= ~termios.ECHO
        termios.tcsetattr(master_fd, termios.TCSANOW, attributes)
        time.sleep(0.1)
        if self._process.poll() is not None:
            raise HalSessionError(
                f"halrun ended during startup: {self._read_log().strip()}"
            )

    def command(self, command: HalCommand) -> None:
        process = self._process
        if process is None or process.poll() is not None:
            raise HalSessionError("halrun is not active")
        if self._master_fd is None:
            raise HalSessionError("halrun pseudo-terminal is unavailable")

        self._command_number += 1
        token = f"__PENDANT_HAL_ACK_{self._command_number}__"
        rendered = _render_hal_command(command)
        try:
            os.write(
                self._master_fd,
                f"{rendered}\nprint {token}\n".encode("utf-8"),
            )
        except BrokenPipeError as error:
            raise HalSessionError(
                f"halrun ended while sending: {rendered}; {self._read_log().strip()}"
            ) from error

        deadline = time.monotonic() + 5.0
        command_output = ""
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise HalSessionError(
                    f"HAL command failed ({rendered}): "
                    f"{command_output.strip()}"
                )
            readable, _, _ = select.select(
                [self._master_fd], [], [], max(0.0, deadline - time.monotonic())
            )
            if not readable:
                continue
            try:
                chunk = os.read(self._master_fd, 4096).decode(
                    "utf-8", errors="replace"
                )
            except OSError:
                chunk = ""
            if not chunk:
                continue
            self._output.append(chunk)
            command_output += chunk
            if f"HALCMD MSG: {token}" in command_output:
                error_lines = [
                    line.strip()
                    for line in command_output.splitlines()
                    if "<stdin>:" in line and "HALCMD MSG:" not in line
                ]
                if error_lines:
                    raise HalSessionError(
                        f"HAL command failed ({rendered}): {'; '.join(error_lines)}"
                    )
                return
        raise HalSessionError(f"HAL command timed out: {rendered}")

    def commands(self, commands: Iterable[HalCommand]) -> None:
        for command in commands:
            self.command(command)

    def _read_log(self) -> str:
        return "".join(self._output)

    @property
    def transcript(self) -> str:
        return self._read_log()

    def close(self) -> None:
        process = self._process
        if process is None:
            return

        if process.poll() is None:
            for command in (("stop",), ("unloadrt", "all")):
                try:
                    self.command(command)
                except (HalSessionError, subprocess.SubprocessError):
                    pass
            try:
                if self._master_fd is not None:
                    os.write(self._master_fd, b"quit\n")
                process.wait(timeout=3)
            except (BrokenPipeError, subprocess.TimeoutExpired):
                process.terminate()
                try:
                    process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=2)
        self._process = None
        if self._master_fd is not None:
            os.close(self._master_fd)
            self._master_fd = None

    def __enter__(self) -> HalSession:
        self.start()
        return self

    def __exit__(self, exc_type, exc_value, traceback) -> None:
        self.close()
