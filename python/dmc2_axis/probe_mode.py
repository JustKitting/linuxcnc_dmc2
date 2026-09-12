"""Stock AXIS widgets only; Rust owns acquisition, records and touch details."""
from __future__ import annotations

import json
import os
from pathlib import Path
import socket
import time

REPLY_RETRY_SECONDS = 1.0
POLL_MS = 50  # matches this profile's DISPLAY CYCLE_TIME
SECTION = "PROBE MODE"


class ProbeModeBinding:
    def __init__(self, namespace):
        self.ns = namespace
        self.root = namespace["root_window"]
        self.tk = self.root.tk
        self.comp = namespace["comp"]
        self.section = None
        self.cursor = 0
        self.state = "connecting"
        self.saved = ""
        self.last_reply = None
        self.socket = None
        self.pending_since = None
        self.controls = {}
        self.message = "Connecting to probe recorder..."
        hal = namespace["hal"]
        for name in ("probe-mode", "probe-record"):
            self.comp.newpin(name, hal.HAL_BIT, hal.HAL_OUT)
            self.comp[name] = False
        self.comp.newpin("probe-retry-save", hal.HAL_U32, hal.HAL_OUT)
        self.comp["probe-retry-save"] = 0
        for name in ("probe-recorder-ready", "probe-selected", "probe-contact", "probe-recording"):
            self.comp.newpin(name, hal.HAL_BIT, hal.HAL_IN)

    def invoke(self, command):
        try:
            command()
        except Exception as error:
            self.message = f"Probe control could not apply the requested change: {error}. Retry the same visible control."
            self.ns["notifications"].add("error", self.message)
            self.render()

    def toggle_mode(self):
        # Off remains reachable independently of recorder/socket/save state.
        enabled = not bool(self.comp["probe-mode"])
        if not enabled:
            self.comp["probe-record"] = False
        self.comp["probe-mode"] = enabled
        self.render()

    def toggle_record(self):
        if self.comp["probe-record"]:
            self.comp["probe-record"] = False
            self.state = "stopping"
        elif self.comp["probe-mode"] and self.state == "idle":
            self.comp["probe-record"] = True
            self.state = "starting"
        self.render()

    def retry_save(self):
        self.comp["probe-retry-save"] = (int(self.comp["probe-retry-save"]) + 1) & 0xFFFFFFFF
        self.state = "saving"
        self.render()

    def install_widgets(self):
        queue = ["."]
        while queue:
            path = queue.pop()
            try:
                label = str(self.tk.call(path, "cget", "-text"))
            except self.ns["Tkinter"].TclError:
                label = ""
            if label == SECTION and str(self.tk.call("winfo", "class", path)) == "Labelframe":
                self.section = path
                break
            queue.extend(self.tk.splitlist(self.tk.call("winfo", "children", path)))
        if self.section is None:
            return
        for child in self.tk.splitlist(self.tk.call("winfo", "children", self.section)):
            self.tk.call("destroy", child)
        bar = self.section + ".controls"
        self.tk.call("frame", bar)
        self.tk.call("pack", bar, "-anchor", "w", "-fill", "x")
        for name, text, action in (
            ("mode", "Probe Mode: OFF", self.toggle_mode),
            ("record", "Record: OFF", self.toggle_record),
            ("retry", "Retry Save", self.retry_save),
        ):
            path = bar + "." + name
            self.controls[name] = path
            self.tk.call("button", path, "-text", text, "-command", self.root.register(lambda command=action: self.invoke(command)), "-takefocus", 0)
            self.tk.call("pack", path, "-side", "left", "-padx", 4)
        for name in ("status", "latest", "saved"):
            path = self.section + "." + name
            self.controls[name] = path
            self.tk.call("label", path, "-text", "", "-anchor", "w", "-justify", "left", "-wraplength", 680)
            self.tk.call("pack", path, "-anchor", "w", "-fill", "x")
        self.render()

    def render(self):
        if self.section is None:
            return
        mode = bool(self.comp["probe-mode"])
        recording = bool(self.comp["probe-record"])
        ready = bool(self.comp["probe-recorder-ready"])
        self.tk.call(self.controls["mode"], "configure", "-text", "Probe Mode: ON" if mode else "Probe Mode: OFF", "-relief", "sunken" if mode else "raised", "-state", "normal" if mode or ready else "disabled")
        self.tk.call(self.controls["record"], "configure", "-text", "Record: ON" if recording else "Record: OFF", "-relief", "sunken" if recording else "raised", "-state", "normal" if recording or (mode and ready and self.state == "idle") else "disabled")
        self.tk.call(self.controls["retry"], "configure", "-state", "normal" if self.state == "save_failed" else "disabled")
        self.tk.call(self.controls["status"], "configure", "-text", self.message)
        self.tk.call(self.controls["saved"], "configure", "-text", ("Saved: " + self.saved) if self.saved else "")

    def receive(self):
        if self.socket is None:
            channel = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
            try:
                channel.setblocking(False)
                channel.bind(f"\0dmc2-probe-ui-{os.getpid()}-{id(self)}")
                channel.connect(str(Path(os.environ["XDG_RUNTIME_DIR"]) / "dmc2-probe.sock"))
            except Exception:
                channel.close()
                raise
            self.socket = channel
        if self.pending_since is not None and time.monotonic() - self.pending_since >= REPLY_RETRY_SECONDS:
            self.socket.close()
            self.socket = None
            self.pending_since = None
            raise RuntimeError("recorder reply missing; reconnecting without changing mode or recording")
        # One request at a time: no unbounded datagram queue if the worker stalls.
        if self.pending_since is None:
            self.socket.send(f"STATUS {self.cursor}".encode("ascii"))
            self.pending_since = time.monotonic()
        try:
            packet = self.socket.recv(16384)
        except BlockingIOError:
            # An outstanding nonblocking request is normal. Preserve the last
            # status until a reply arrives or the deadline above reports an
            # actual timeout, rather than flashing a waiting message per poll.
            return
        self.pending_since = None
        status = json.loads(packet)
        self.last_reply = time.monotonic()
        self.state = status["state"]
        self.saved = status["saved"]
        if status["error"]:
            self.message = status["error"]
        elif self.state == "recording":
            self.message = f"Recording in memory: {status['touches']} touches, {status['samples']} movement samples. Turn Record off to save."
        elif self.state == "saving":
            self.message = "Saving recording..."
        elif self.comp["probe-mode"]:
            self.message = ("Probe held" if self.comp["probe-contact"] else "Probe released") + (" · touch bubbles only" if self.comp["probe-selected"] else " · return to Manual/idle to select the XYZ probe")
        else:
            self.message = "Probe Mode off"
        if self.section is not None:
            self.tk.call(self.controls["latest"], "configure", "-text", status["latest"])
        if int(status["touch_id"]) > self.cursor:
            if self.comp["probe-mode"]:
                if self.ns["notifications"].add("info", status["bubble"]) is False:
                    raise RuntimeError("touch bubble delivery failed; touch remains queued")
            self.cursor = int(status["touch_id"])

    def poll(self):
        try:
            if self.section is None:
                self.install_widgets()
            self.receive()
        except Exception as error:
            self.message = f"Probe display unavailable: {error}. Probe Mode off remains available; reconnecting the display."
            if self.socket is not None:
                self.socket.close()
                self.socket = None
            self.pending_since = None
        finally:
            try:
                self.render()
            except Exception as error:
                self.ns["notifications"].add("error", f"Probe section refresh failed: {error}. The toolbar Clear Fault and Pendant Mode controls remain independent.")
            finally:
                self.root.after(POLL_MS, self.poll)


def install_axis_probe_mode(namespace):
    binding = ProbeModeBinding(namespace)
    namespace["live_plotter"]._dmc2_probe_mode = binding
    binding.poll()
    return binding
