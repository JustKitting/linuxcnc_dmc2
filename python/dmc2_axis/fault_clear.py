"""Stock AXIS presentation for the Rust operator Clear Fault operation."""
from __future__ import annotations

from pathlib import Path
import os
import signal
import subprocess

from .constants import CLEAR_FAULT_WIDGET_PATH, HOMING_STATE_POLL_MILLISECONDS
from .recovery_ui import RecoveryUiNotice
from .ui_fault import AxisUiFault, AxisUiFaultKind


class ClearFaultBinding:
    def __init__(self, namespace):
        self.namespace = namespace
        self.root = namespace["root_window"]
        self.project = Path(str(namespace["rcfile"])).resolve().parents[2]
        self.process = None
        self.superseded = []
        self.after_id = None
        self.notice = RecoveryUiNotice(namespace)

    def __call__(self):
        try:
            cancellation_error = None
            # Every click takes priority. End only the previous clear helper's
            # private process group, including its HAL child; never LinuxCNC.
            if self.process is not None:
                previous, self.process = self.process, None
                if previous.poll() is None:
                    try:
                        os.killpg(previous.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass  # It exited between poll and the cancellation.
                    except OSError as error:
                        # Even cancellation failure cannot veto the new clear.
                        # Its request number supersedes the older Rust worker.
                        cancellation_error = error
                self.superseded.append(previous)
            self.process = subprocess.Popen(
                [str(self.project / "native/bin/dmc2ctl"),
                 "clear-fault"],
                cwd=self.project,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                start_new_session=True,
            )
            print("DMC2_CLEAR_FAULT_UI submitted=operator-request", flush=True)
            if cancellation_error is not None:
                self.notice.present(fault=AxisUiFault(
                    AxisUiFaultKind.CLEAR_FAULT_UI_COMMAND_FAILED,
                    f"Previous clear helper could not be cancelled: {cancellation_error}. The new Clear Fault request was submitted."))
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
        remaining = []
        for previous in self.superseded:
            if previous.poll() is None:
                remaining.append(previous)
            else:
                output, _ = previous.communicate()
                print(f"DMC2_CLEAR_FAULT_UI superseded={previous.pid} result={output!r}", flush=True)
        self.superseded = remaining
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
            for line in output.splitlines():
                if line.startswith("OPERATOR_MESSAGE="):
                    self.namespace["notifications"].add("info", line.partition("=")[2])
                elif line.startswith("OPERATOR_WARNING="):
                    self.namespace["notifications"].add("error", line.partition("=")[2])
