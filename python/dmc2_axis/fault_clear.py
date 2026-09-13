"""Stock AXIS presentation for the Rust operator Clear Fault operation."""
from __future__ import annotations

from pathlib import Path
import subprocess

from .constants import CLEAR_FAULT_OPERATION_ID, CLEAR_FAULT_WIDGET_PATH, HOMING_STATE_POLL_MILLISECONDS
from .recovery_ui import RecoveryUiNotice
from .ui_fault import AxisUiFault, AxisUiFaultKind


class ClearFaultBinding:
    def __init__(self, namespace):
        self.namespace = namespace
        self.root = namespace["root_window"]
        self.project = Path(str(namespace["rcfile"])).resolve().parents[2]
        self.process = None
        self.after_id = None
        self.notice = RecoveryUiNotice(namespace)

    def __call__(self):
        # Keep every UI control available; repeated clicks never enqueue resets.
        if self.process is not None and self.process.poll() is None:
            return
        if self.process is not None:
            self.poll()
        try:
            self.process = subprocess.Popen(
                [str(self.project / "native/bin/dmc2ctl"),
                 "--catalog", str(self.project / "config/operations.tsv"),
                 "execute", CLEAR_FAULT_OPERATION_ID],
                cwd=self.project,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
            )
            print("DMC2_CLEAR_FAULT_UI submitted=operator-request", flush=True)
            self.root.tk.call(CLEAR_FAULT_WIDGET_PATH, "configure", "-text", "CLEARING...")
            self.poll()
        except Exception as error:
            self.notice.present(fault=AxisUiFault(
                AxisUiFaultKind.CLEAR_FAULT_UI_COMMAND_FAILED, error))

    def poll(self):
        try:
            self._poll()
        except Exception as error:
            self.notice.present(fault=AxisUiFault(
                AxisUiFaultKind.CLEAR_FAULT_UI_COMMAND_FAILED, error))

    def _scheduled_poll(self):
        self.after_id = None
        self.poll()

    def _poll(self):
        if self.process is None:
            return
        if self.process.poll() is None:
            if self.after_id is None:
                self.after_id = self.root.after(HOMING_STATE_POLL_MILLISECONDS, self._scheduled_poll)
            return
        process, self.process = self.process, None
        output, _ = process.communicate()
        self.root.tk.call(CLEAR_FAULT_WIDGET_PATH, "configure", "-text", "CLEAR FAULT")
        print(f"DMC2_CLEAR_FAULT_UI exit={process.returncode} result={output!r}", flush=True)
        if process.returncode:
            self.notice.present(fault=AxisUiFault(
                AxisUiFaultKind.CLEAR_FAULT_UI_COMMAND_FAILED, output.strip()))
        else:
            self.notice.clear()
