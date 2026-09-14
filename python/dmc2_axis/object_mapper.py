"""AXIS operator access to Rust object-map operations; no machine state ownership."""
from __future__ import annotations

from pathlib import Path
import tkinter as tk
from tkinter import ttk

from .constants import HOMING_STATE_POLL_MILLISECONDS
from .mapper_jobs import MapperJob, Operation, FieldKind, Outcome
from .mapper_widgets import DocumentView, PathChooser, recovery_controls


class ObjectMapperBinding:
    def __init__(self, namespace):
        self.root = namespace["root_window"]
        self.project = Path(str(namespace["rcfile"])).resolve().parents[2]
        self.window = None
        self.job = None
        self.operations = {}
        self.values = {}
        self.fields = {}
        self.objects = {}
        self.browser = None
        self.pending = None
        tabs = str(namespace["vcp_frame"]) + ".dmc2_tabs"
        frame = str(self.root.tk.call(tabs, "insert", "end", "mapper", "-text", "Object Mapper"))
        self.root.tk.call("button", frame + ".open", "-text", "Open object mapper", "-command", self.root.register(self.open))
        self.root.tk.call("pack", frame + ".open", "-fill", "x")
        self.root.tk.call("label", frame + ".description", "-text",
                          "Retained captures, model placement and stock measurements.", "-justify", "left")
        self.root.tk.call("pack", frame + ".description", "-fill", "x")
        self.root.tk.call("bind", frame + ".description", "<Configure>", f"{frame}.description configure -wraplength [expr {{max(1, %w - 8)}}]")
        self.root.tk.call(tabs, "compute_size")

    def open(self):
        try:
            if self.window is None:
                self.create_window()
                self.load_catalog()
            self.window.deiconify()
            self.window.lift()
        except Exception as error:
            if self.window is not None:
                self.fail(f"Could not open mapper: {error}. Retry Reload operations.")
            else:
                raise

    def create_window(self):
        self.window = tk.Toplevel(self.root)
        self.window.title("DMC2 Object Mapper")
        self.window.protocol("WM_DELETE_WINDOW", self.window.withdraw)
        toolbar = recovery_controls(self.root, self.window)
        ttk.Button(toolbar, text="Hide mapper", command=self.window.withdraw).pack(side="right")
        ttk.Button(toolbar, text="Reload operations", command=self.load_catalog).pack(side="left")
        self.status = ttk.Label(self.window, text="Loading operations…", wraplength=600)
        self.status.pack(fill="x")
        self.status.bind("<Configure>", lambda event: self.status.configure(wraplength=max(1, event.width)))
        self.selected = tk.StringVar(self.window)
        self.menu = ttk.Combobox(self.window, textvariable=self.selected, state="readonly")
        self.menu.pack(fill="x")
        self.menu.bind("<<ComboboxSelected>>", lambda _: self.select())
        self.description = ttk.Label(self.window, wraplength=600)
        self.description.pack(fill="x")
        self.description.bind("<Configure>", lambda event: self.description.configure(wraplength=max(1, event.width)))
        self.form = ttk.Frame(self.window)
        self.form.pack(fill="x")
        self.form.columnconfigure(1, weight=1)
        actions = ttk.Frame(self.window)
        actions.pack(fill="x")
        self.run_button = ttk.Button(actions, text="Run selected operation", command=self.run, state="disabled")
        self.run_button.pack(side="left")
        ttk.Button(actions, text="Cancel analysis", command=self.cancel).pack(side="left")
        self.documents = DocumentView(self.window)
        self.documents.pack(fill="both", expand=True)
        self.window.geometry(f"{self.root.winfo_width()}x{self.root.winfo_height()}")

    def fail(self, message):
        self.status.configure(text=message)

    def value(self, key):
        if key not in self.values:
            self.values[key] = tk.StringVar(self.window)
        return self.values[key]

    def choices(self, kind):
        obj = self.objects.get(self.value("object").get(), {})
        if kind is FieldKind.OBJECT:
            return tuple(self.objects)
        if kind is FieldKind.SETUP:
            return tuple(s["id"] for s in obj.get("setups", []))
        if kind is FieldKind.DESIGN:
            return tuple(d["revision"] for d in obj.get("design_revisions", []) if d["format"] == "stl")
        if kind is FieldKind.ANALYSIS:
            setup = next((s for s in obj.get("setups", []) if s["id"] == self.value("setup").get()), {})
            return tuple(a["id"] for a in setup.get("analysis_candidates", []))
        return ()

    def select(self):
        try:
            operation = self.operations[self.selected.get()]
            for child in self.form.winfo_children():
                child.destroy()
            self.fields = {}
            self.description.configure(text=operation.description)
            for row, field in enumerate(operation.fields):
                ttk.Label(self.form, text=field.label).grid(row=row, column=0, sticky="w")
                entry = ttk.Combobox(self.form, textvariable=self.value(field.key), values=self.choices(field.kind))
                entry.configure(postcommand=self.refresh_choices)
                entry.grid(row=row, column=1, sticky="ew")
                entry.bind("<<ComboboxSelected>>", lambda _: self.refresh_choices())
                self.fields[field.key] = (field, entry)
                if field.kind in (FieldKind.FILE, FieldKind.DIRECTORY, FieldKind.NEW_PATH):
                    ttk.Button(self.form, text="Browse", command=lambda field=field: self.browse(field)).grid(row=row, column=2)
            self.update_actions()
        except Exception as error:
            self.fail(f"Cannot display operation: {error}. Reload operations to retry.")

    def refresh_choices(self):
        for field, entry in self.fields.values():
            entry.configure(values=self.choices(field.kind))

    def browse(self, field):
        try:
            variable = self.value(field.key)
            # Reuse a nonmodal chooser; hiding it retains its current directory.
            if self.browser is None:
                self.browser = PathChooser(self.root, self.project, variable.set, str(self.project))
            else:
                self.browser.selected = variable.set
                self.browser.window.deiconify()
                self.browser.window.lift()
        except Exception as error:
            self.fail(f"Could not open path chooser: {error}. Enter the path in the field or retry Browse.")

    def update_actions(self):
        self.run_button.configure(state="normal" if self.job is None and self.selected.get() in self.operations else "disabled")

    def submit(self, arguments, kind, pending, request=None):
        if self.job is not None:
            self.fail("A mapper command is running. Use Cancel analysis or wait for its result.")
            return
        self.job = MapperJob(self.project, arguments, kind, request)
        self.pending = pending
        try:
            self.job.start()
        except Exception as error:
            self.job = None
            self.fail(f"Could not start mapper command: {error}. Retry the selected operation.")
            self.update_actions()
            return
        self.status.configure(text="Reading or calculating retained object data…")
        self.update_actions()
        self.poll()

    def load_catalog(self):
        if self.window is not None:
            self.submit(("catalog",), "json", None)

    def run(self):
        try:
            operation = self.operations[self.selected.get()]
            values = [self.value(field.key).get() for field in operation.fields]
            missing = [field.label for field, value in zip(operation.fields, values) if not value.strip()]
            if missing:
                raise ValueError("Fill " + ", ".join(missing) + ", then retry.")
            request = self.documents.request() if operation.input == "request" else None
            self.submit((operation.command, *values), operation.result, operation, request)
        except Exception as error:
            self.fail(f"Could not start selected operation: {error}. Correct the fields or Reload operations, then retry.")

    def cancel(self):
        if self.job is not None:
            try:
                self.job.cancel()
                self.status.configure(text="Cancellation requested for the offline analysis command.")
            except Exception as error:
                self.fail(f"Could not cancel analysis: {error}. Retry Cancel analysis; Clear Fault and Pendant Mode remain independent.")

    def poll(self):
        result = self.job.take_result()
        if result is None:
            self.root.after(HOMING_STATE_POLL_MILLISECONDS, self.poll)
            return
        operation = self.pending
        self.job = None
        try:
            if result.outcome is not Outcome.RESULT:
                self.fail(result.message)
            elif operation is None:
                self.apply_catalog(result.data)
            else:
                if isinstance(result.data, dict):
                    data = result.data
                    if data.get("schema") == "dmc2.objects.v1":
                        self.objects.update((item["id"], {**self.objects.get(item["id"], {}), **item}) for item in data["objects"])
                    elif data.get("schema") == "dmc2.object.v1":
                        self.objects[data["id"]] = data
                    self.refresh_choices()
                self.documents.add(operation.label, result.documents)
                self.status.configure(text=result.message)
        except Exception as error:
            self.fail(f"Could not present mapper result: {error}. Retry the operation or Reload operations; retained files are preserved.")
        finally:
            self.update_actions()

    def apply_catalog(self, data):
        if data.get("schema") != "dmc2.object-map-operations.v1":
            raise ValueError("The installed binary returned an unsupported mapper catalog.")
        operations = tuple(Operation.read(row) for row in data["operations"])
        if len({item.label for item in operations}) != len(operations) or len({item.command for item in operations}) != len(operations):
            raise ValueError("Mapper command identities are duplicated.")
        self.operations = {item.label: item for item in operations}
        self.menu.configure(values=tuple(self.operations))
        initial = next(item for item in operations if item.command == data["initial_command"])
        self.selected.set(initial.label)
        self.select()
        self.status.configure(text="Choose an operation. Start with List objects or Create object.")


def install_axis_object_mapper(namespace):
    binding = ObjectMapperBinding(namespace)
    namespace["live_plotter"]._dmc2_object_mapper = binding
    return binding
