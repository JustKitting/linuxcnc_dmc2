#!/usr/bin/python3
"""Disabled historical operator confirmation UI retained as a test oracle."""

from __future__ import annotations

import math
import pathlib
import subprocess
import tkinter as tk
from collections.abc import Callable
from tkinter import messagebox

import linuxcnc


PROGRAM = (
    pathlib.Path(__file__).resolve().parents[3]
    / "live"
    / "nc_files"
    / "tool-height-homing-style-test.ngc"
)
EXPECTED_X = 288.125
EXPECTED_Y = 152.955
EXPECTED_Z = 135.0
POSITION_TOLERANCE = 0.010


def read_hal_bit(pin: str) -> bool:
    result = subprocess.run(
        ["halcmd", "getp", pin],
        check=True,
        capture_output=True,
        text=True,
    )
    value = result.stdout.strip()
    if value == "TRUE":
        return True
    if value == "FALSE":
        return False
    raise RuntimeError(f"Unexpected value for {pin}: {value!r}")


def read_refusal_reason(status: linuxcnc.stat) -> str | None:
    status.poll()

    if not status.paused:
        return "The tool-check program is not paused at its confirmation gate."
    if pathlib.Path(status.file or "") != PROGRAM:
        return "A different LinuxCNC program is loaded."
    if not status.enabled or status.estop:
        return "LinuxCNC is not enabled or emergency stop is active."
    if not all(status.homed[:3]):
        return "X, Y, and Z are not all homed."
    if not math.isclose(
        status.actual_position[0], EXPECTED_X, abs_tol=POSITION_TOLERANCE
    ):
        return "X is no longer at the recorded puck position."
    if not math.isclose(
        status.actual_position[1], EXPECTED_Y, abs_tol=POSITION_TOLERANCE
    ):
        return "Y is no longer at the recorded puck position."
    if not math.isclose(
        status.actual_position[2], EXPECTED_Z, abs_tol=POSITION_TOLERANCE
    ):
        return "Z is no longer at known machine home."
    if any(abs(value) > 1e-9 for value in status.g5x_offset[:3]):
        return "The active work offset is no longer zero."
    if any(abs(value) > 1e-9 for value in status.g92_offset[:3]):
        return "A G92 offset is active."
    if any(abs(value) > 1e-9 for value in status.tool_offset[:3]):
        return "A tool offset is active."
    if status.probe_val:
        return "IN0 is already active before the tool check."
    if status.spindle[0]["enabled"]:
        return "The spindle is running."
    if read_hal_bit("motion.digital-out-00"):
        return "The LinuxCNC OUT5 request is already active."
    if read_hal_bit("motion.digital-in-01"):
        return "The OUT5 gate reports active."
    if read_hal_bit("hm2_7i95.0.ssr.00.out-05"):
        return "Mesa OUT5 is already active."
    if read_hal_bit("hm2_7i95.0.watchdog.has_bit"):
        return "The Mesa watchdog has bitten."
    if read_hal_bit("hm2_7i95.0.packet-error"):
        return "Mesa reports a packet error."
    return None


class ConfirmationWindow:
    def __init__(self) -> None:
        self.status = linuxcnc.stat()
        self.command = linuxcnc.command()
        self.errors = linuxcnc.error_channel()
        self.latest_message = ""
        self.root = tk.Tk()
        self.root.withdraw()
        self.popup: tk.Toplevel | None = None
        self.heading: tk.Label | None = None
        self.instructions: tk.Label | None = None
        self.start_button: tk.Button | None = None
        self.connection_started = False
        self.connection_left_first_gate = False
        self.measurement_started = False

    def _show_popup(
        self,
        *,
        title: str,
        heading: str,
        instructions: str,
        start_label: str,
        start_command: Callable[[], None],
        cancel_label: str,
    ) -> None:
        if self.popup is not None:
            self.popup.destroy()

        self.popup = tk.Toplevel(self.root)
        self.popup.title(title)
        self.popup.resizable(False, False)
        self.popup.attributes("-topmost", True)
        self.popup.protocol("WM_DELETE_WINDOW", self.cancel)
        self.popup.bind("<Escape>", lambda _event: self.cancel())
        self.popup.bind("<Return>", lambda _event: start_command())

        body = tk.Frame(self.popup, padx=24, pady=20)
        body.pack(fill="both", expand=True)
        self.heading = tk.Label(
            body,
            text=heading,
            font=("Helvetica", 16, "bold"),
            anchor="w",
        )
        self.heading.pack(fill="x", pady=(0, 12))
        self.instructions = tk.Label(
            body,
            justify="left",
            anchor="w",
            text=instructions,
            font=("Helvetica", 12),
        )
        self.instructions.pack(fill="x")

        buttons = tk.Frame(body)
        buttons.pack(fill="x", pady=(20, 0))
        tk.Button(
            buttons,
            text=cancel_label,
            command=self.cancel,
            padx=14,
            pady=8,
        ).pack(side="left")
        self.start_button = tk.Button(
            buttons,
            text=start_label,
            command=start_command,
            padx=14,
            pady=8,
            default="active",
        )
        self.start_button.pack(side="right")

        self.popup.update_idletasks()
        width = self.popup.winfo_reqwidth()
        height = self.popup.winfo_reqheight()
        x = (self.popup.winfo_screenwidth() - width) // 2
        y = (self.popup.winfo_screenheight() - height) // 2
        self.popup.geometry(f"+{x}+{y}")
        self.popup.lift()
        self.popup.focus_force()

    def show_connection_popup(self) -> None:
        self._show_popup(
            title="DMC2 Tool Check — Connection Test",
            heading="1. CONNECTION TEST — NO MOTION",
            instructions=(
                "The program is PAUSED. OUT5 is OFF. No axis will move.\n\n"
                "Hold the 19.40 mm puck free; do not place it under the tool yet.\n"
                "Attach the clip, then press START CONNECTION TEST.\n\n"
                "OUT5 will turn on and the program will wait for you to manually\n"
                "touch the puck to the installed tool tip. A real IN0 contact is\n"
                "required. Once detected, OUT5 turns off and the placement popup opens."
            ),
            start_label="START CONNECTION TEST — NO MOTION",
            start_command=self.start_connection_test,
            cancel_label="CANCEL — NO MOTION",
        )

    def start_connection_test(self) -> None:
        if self.connection_started:
            return

        refusal = read_refusal_reason(self.status)
        if refusal is not None:
            messagebox.showerror("Tool check refused", refusal, parent=self.popup)
            return

        self.connection_started = True
        self.connection_left_first_gate = False
        assert self.popup is not None
        assert self.heading is not None
        assert self.instructions is not None
        assert self.start_button is not None
        self.popup.unbind("<Return>")
        self.heading.configure(text="CONNECTION TEST ACTIVE — NO MOTION")
        self.instructions.configure(
            text=(
                "OUT5 is being enabled for the electrical test only.\n"
                "No axis-motion command exists in this stage.\n\n"
                "Touch the 19.40 mm puck to the installed tool tip now.\n"
                "Waiting for a real IN0 contact..."
            )
        )
        self.start_button.configure(text="WAITING FOR IN0 CONTACT", state="disabled")
        self.command.auto(linuxcnc.AUTO_RESUME)
        self.root.after(25, self.poll_connection_test)

    def _poll_errors(self) -> None:
        while True:
            message = self.errors.poll()
            if message is None:
                return
            self.latest_message = str(message[1])

    def poll_connection_test(self) -> None:
        self.status.poll()
        self._poll_errors()

        if self.status.paused:
            if not self.connection_left_first_gate:
                self.root.after(25, self.poll_connection_test)
                return
            refusal = read_refusal_reason(self.status)
            if refusal is not None:
                self.command.abort()
                self.command.wait_complete(5.0)
                messagebox.showerror(
                    "Connection test failed", refusal, parent=self.popup
                )
                self.root.destroy()
                return
            self.show_measurement_popup()
            return

        if self.status.interp_state == linuxcnc.INTERP_IDLE:
            detail = self.latest_message or "The connection test ended without IN0 contact."
            messagebox.showerror(
                "Connection test failed", detail, parent=self.popup
            )
            self.root.destroy()
            return

        self.connection_left_first_gate = True
        self.root.after(25, self.poll_connection_test)

    def show_measurement_popup(self) -> None:
        self._show_popup(
            title="DMC2 Tool Check — Place Puck and Measure",
            heading="2. CONNECTION PASSED — PLACE PUCK",
            instructions=(
                "IN0 contact was detected. OUT5 is OFF and the program is PAUSED.\n\n"
                "Place the 19.40 mm puck under the installed tool and keep the clip attached.\n\n"
                "Pressing START TOOL MEASURE begins motion immediately:\n"
                "  Z probes down at exactly 5 mm/s.\n"
                "  Z backs off upward 1.00 mm at exactly 0.25 mm/s.\n"
                "  Z re-touches downward at exactly 0.25 mm/s.\n"
                "  OUT5 turns off before the accelerated return to Z home.\n\n"
                "Keep your kill switch ready."
            ),
            start_label="START TOOL MEASURE",
            start_command=self.start_measurement,
            cancel_label="CANCEL — NO TOOL MOTION",
        )

    def start_measurement(self) -> None:
        if self.measurement_started:
            return

        refusal = read_refusal_reason(self.status)
        if refusal is not None:
            messagebox.showerror("Tool measure refused", refusal, parent=self.popup)
            return

        self.measurement_started = True
        assert self.popup is not None
        assert self.start_button is not None
        self.popup.unbind("<Return>")
        self.start_button.configure(state="disabled")
        self.command.auto(linuxcnc.AUTO_RESUME)
        self.root.destroy()

    def cancel(self) -> None:
        self.command.abort()
        self.command.wait_complete(5.0)
        self.root.destroy()

    def run(self) -> None:
        refusal = read_refusal_reason(self.status)
        if refusal is not None:
            messagebox.showerror("Tool check refused", refusal, parent=self.root)
            self.root.destroy()
            return
        self.show_connection_popup()
        self.root.mainloop()


if __name__ == "__main__":
    raise SystemExit(
        "LEGACY PROBE COMMAND UI DISABLED: use LinuxCNC's native program and "
        "operator controls; this Python file has no live command authority."
    )
