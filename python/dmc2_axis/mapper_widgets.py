"""Nonmodal, bounded-size AXIS views for mapper files and draft text."""
from __future__ import annotations

from dataclasses import dataclass
import tkinter as tk
from tkinter import ttk

from .constants import CLEAR_FAULT_WIDGET_PATH, PENDANT_WIDGET_PATH, HOMING_STATE_POLL_MILLISECONDS
from .mapper_jobs import MapperJob, Outcome

# Limit synchronous Tk text insertion, never the retained measurements or file.
PAGE_CHARACTERS = 16 * 1024
BROWSER_ROWS = 200


def recovery_controls(root, parent):
    row = ttk.Frame(parent)
    row.pack(fill="x")
    for label, path in (("CLEAR FAULT", CLEAR_FAULT_WIDGET_PATH), ("Pendant Mode", PENDANT_WIDGET_PATH)):
        ttk.Button(row, text=label, command=lambda path=path: root.tk.call(path, "invoke")).pack(side="left")
    return row


@dataclass
class Document:
    text: str
    editable: bool


class DocumentView(ttk.Frame):
    def __init__(self, parent):
        super().__init__(parent)
        self.documents = {}
        self.current = None
        self.offset = 0
        self.shown_length = 0
        self.serial = 0
        row = ttk.Frame(self)
        row.pack(fill="x")
        self.selection = tk.StringVar(self)
        self.choices = ttk.Combobox(row, textvariable=self.selection, state="readonly")
        self.choices.pack(side="left", fill="x", expand=True)
        self.choices.bind("<<ComboboxSelected>>", lambda _: self.select())
        ttk.Button(row, text="Previous", command=lambda: self.page(-1)).pack(side="left")
        ttk.Button(row, text="Next", command=lambda: self.page(1)).pack(side="left")
        self.position = ttk.Label(self)
        self.position.pack(fill="x")
        body = ttk.Frame(self)
        body.pack(fill="both", expand=True)
        self.text = tk.Text(body, width=1, height=12, wrap="none", state="disabled")
        sy = ttk.Scrollbar(body, orient="vertical", command=self.text.yview)
        sx = ttk.Scrollbar(self, orient="horizontal", command=self.text.xview)
        self.text.configure(yscrollcommand=sy.set, xscrollcommand=sx.set)
        sy.pack(side="right", fill="y")
        self.text.pack(side="left", fill="both", expand=True)
        sx.pack(fill="x")

    def commit(self):
        if self.current is not None and self.documents[self.current].editable:
            doc = self.documents[self.current]
            page = self.text.get("1.0", "end-1c")
            doc.text = doc.text[:self.offset] + page + doc.text[self.offset + self.shown_length:]
            self.shown_length = len(page)

    def add(self, operation, documents):
        self.commit()
        self.serial += 1
        added = []
        for label, text, editable in documents:
            key = f"{self.serial}: {operation} — {label}"
            self.documents[key] = Document(text, editable)
            added.append(key)
        self.choices.configure(values=tuple(self.documents))
        if documents:
            self.selection.set(added[0])
            self.select()

    def select(self):
        self.commit()
        self.current = self.selection.get()
        self.offset = 0
        self.show()

    def show(self):
        if self.current is None:
            return
        doc = self.documents[self.current]
        page = doc.text[self.offset:self.offset + PAGE_CHARACTERS]
        self.shown_length = len(page)
        self.text.configure(state="normal")
        self.text.delete("1.0", "end")
        self.text.insert("1.0", page)
        self.text.configure(state="normal" if doc.editable else "disabled")
        self.position.configure(text=f"Characters {self.offset + 1}–{self.offset + len(page)} of {len(doc.text)}; " + ("editable draft" if doc.editable else "retained result"))

    def page(self, direction):
        self.commit()
        if self.current is not None:
            length = len(self.documents[self.current].text)
            target = self.offset + self.shown_length if direction > 0 else max(0, self.offset - PAGE_CHARACTERS)
            if target < length:
                self.offset = target
            self.show()

    def request(self):
        self.commit()
        if self.current is None or not self.documents[self.current].editable:
            raise ValueError("Select an editable Request draft in the result selector before saving. Use Prepare fit request or Open fit request to create one.")
        return self.documents[self.current].text


class PathChooser:
    """File selection without a grab or nested wait that captures machine controls."""
    def __init__(self, root, project, selected, initial):
        self.root, self.project, self.selected = root, project, selected
        self.job = None
        self.entries = []
        self.offset = 0
        self.parent_path = str(project)
        self.window = tk.Toplevel(root)
        self.window.title("Choose mapper path")
        row = recovery_controls(root, self.window)
        ttk.Button(row, text="Hide", command=self.window.withdraw).pack(side="right")
        self.window.protocol("WM_DELETE_WINDOW", self.window.withdraw)
        self.path = tk.StringVar(self.window, value=initial or str(project))
        entry = ttk.Entry(self.window, textvariable=self.path)
        entry.pack(fill="x")
        actions = ttk.Frame(self.window)
        actions.pack(fill="x")
        for label, callback in (
            ("List directory", self.browse), ("Parent", self.parent),
            ("Use path", self.use), ("Cancel listing", self.cancel),
        ):
            ttk.Button(actions, text=label, command=callback).pack(side="left")
        self.status = ttk.Label(self.window, wraplength=480)
        self.status.pack(fill="x")
        self.listing = tk.Listbox(self.window, width=65, height=14, exportselection=False)
        self.listing.pack(fill="both", expand=True)
        self.listing.bind("<Double-Button-1>", self.open_selected)
        navigation = ttk.Frame(self.window)
        navigation.pack(fill="x")
        ttk.Button(navigation, text="Previous", command=lambda: self.page(-1)).pack(side="left")
        ttk.Button(navigation, text="Next", command=lambda: self.page(1)).pack(side="left")
        ttk.Button(navigation, text="Select listed path", command=self.choose_selected).pack(side="left")
        self.browse()

    def browse(self):
        if self.job is not None:
            self.status.configure(text="Directory listing is running. Cancel listing remains available.")
            return
        self.job = MapperJob(self.project, ("browse", self.path.get()))
        try:
            self.job.start()
        except Exception as error:
            self.job = None
            self.status.configure(text=f"Cannot start directory listing: {error}. Retry List directory.")
            return
        self.status.configure(text="Reading directory…")
        self.poll()

    def poll(self):
        result = self.job.take_result()
        if result is None:
            self.root.after(HOMING_STATE_POLL_MILLISECONDS, self.poll)
            return
        self.job = None
        try:
            if result.outcome is not Outcome.RESULT:
                self.status.configure(text=result.message)
                return
            self.entries = result.data["entries"]
            self.path.set(result.data["directory"])
            self.parent_path = result.data["parent"]
            self.offset = 0
            self.render()
        except Exception as error:
            self.status.configure(text=f"Cannot display directory: {error}. Edit the path and retry List directory.")

    def cancel(self):
        if self.job is not None:
            try:
                self.job.cancel()
            except Exception as error:
                self.status.configure(text=f"Could not cancel this listing: {error}. Retry Cancel listing; machine controls remain independent.")

    def parent(self):
        self.path.set(self.parent_path)
        self.browse()

    def use(self):
        self.selected(self.path.get())
        self.window.withdraw()

    def choose_selected(self):
        selection = self.listing.curselection()
        if selection:
            self.path.set(self.entries[self.offset + selection[0]]["path"])

    def open_selected(self, _=None):
        selection = self.listing.curselection()
        if selection:
            item = self.entries[self.offset + selection[0]]
            self.path.set(item["path"])
            if item["directory"]:
                self.browse()
            else:
                self.use()

    def page(self, direction):
        self.offset = max(0, min(self.offset + direction * BROWSER_ROWS, max(0, ((len(self.entries) - 1) // BROWSER_ROWS) * BROWSER_ROWS)))
        self.render()

    def render(self):
        self.listing.delete(0, "end")
        for entry in self.entries[self.offset:self.offset + BROWSER_ROWS]:
            self.listing.insert("end", entry["name"] + ("/" if entry["directory"] else ""))
        self.status.configure(text=f"Showing entries {self.offset + 1}–{min(self.offset + BROWSER_ROWS, len(self.entries))} of {len(self.entries)}. Enter a full path or select a listed entry.")
